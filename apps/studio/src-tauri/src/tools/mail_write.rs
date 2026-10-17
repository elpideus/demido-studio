//! Mail tools that change mail: mark (read, starred), move or copy, delete, manage folders,
//! write drafts and send. Each asks the person itself when the permission mode says to (see
//! [`super::permissions`]), with a card that shows the emails or the message rather than the
//! raw arguments; what a call risks can depend on them (deleting for good is riskier than into
//! the trash).
//!
//! Message ids come from `mail_list`, `mail_search` and `mail_export`, given in the call or, for
//! many, in a workspace file (an export the model filtered, say). An id names one message for
//! good: a moved message gets a new id in its new folder, which the results say, while every
//! other message keeps its own, so an export stays good for the rest of the work.

use std::path::Path;

use serde_json::{Value, json};

use super::mail::{ACCOUNT_HELP, err, message_id};
use super::{Risk, ToolContext, ToolOutput, arg_str, require_str, workspace_path};
use crate::agent::Cancelled;
use crate::mail::{
    Account, Changed, Compose, Deleted, Draft, FLAGGED, MAX_ATTACHMENTS, MailService, OutFile, SEEN, mime_of,
};

/// The most emails one call changes.
const MAX_IDS: usize = 5000;
/// The emails named on a card.
const SAMPLE: usize = 8;
const IDS_HELP: &str = "Message ids from mail_list, mail_search or mail_export, like m123";
const IDS_FILE_HELP: &str = "Instead of (or as well as) ids: a workspace file with them, like an export filtered by \
    run_python: JSON Lines with an \"id\" on each line, a JSON list, or ids separated by spaces, commas or lines. \
    Up to 5000 emails in all";

pub fn mark_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ids": {"type": "array", "items": {"type": "string"}, "description": IDS_HELP},
            "ids_file": {"type": "string", "description": IDS_FILE_HELP},
            "read": {"type": "boolean", "description": "true marks read, false unread"},
            "starred": {"type": "boolean", "description": "true stars (flags), false unstars"}
        }
    })
}

pub fn move_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ids": {"type": "array", "items": {"type": "string"}, "description": IDS_HELP},
            "ids_file": {"type": "string", "description": IDS_FILE_HELP},
            "folder": {"type": "string", "description": "Destination folder in the emails' own account: a folder or label name, or a role: inbox, archive, spam, trash…"},
            "copy": {"type": "boolean", "description": "Copy instead of moving (on Gmail: add the label and keep the others)"}
        },
        "required": ["folder"]
    })
}

pub fn delete_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ids": {"type": "array", "items": {"type": "string"}, "description": IDS_HELP},
            "ids_file": {"type": "string", "description": IDS_FILE_HELP},
            "permanent": {"type": "boolean", "description": "Delete for good instead of moving to the trash. Emails already in the trash are always deleted for good"}
        }
    })
}

pub fn folder_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {"type": "string", "enum": ["create", "rename", "delete"]},
            "folder": {"type": "string", "description": "The folder to rename or delete"},
            "name": {"type": "string", "description": "Name of the new folder, or the folder's new name. Use / for subfolders, like Clients/2026"},
            "parent": {"type": "string", "description": "Create inside this folder"},
            "account": {"type": "string", "description": ACCOUNT_HELP}
        },
        "required": ["action"]
    })
}

fn compose_properties() -> serde_json::Map<String, Value> {
    let addresses = |what: &str| json!({"type": "array", "items": {"type": "string"}, "description": format!("{what}: addresses like ada@example.com or Ada <ada@example.com>")});
    let Value::Object(map) = json!({
        "to": addresses("Recipients (a reply defaults to the sender, or the Reply-To address)"),
        "cc": addresses("Copy"),
        "bcc": addresses("Blind copy"),
        "subject": {"type": "string", "description": "Default for a reply or forward: Re: or Fwd: and the original subject"},
        "body": {"type": "string", "description": "Plain text. A reply's quote or a forward's original is added below it"},
        "attachments": {"type": "array", "items": {"type": "string"}, "description": "Workspace files to attach (25 MB in all)"},
        "reply_to": {"type": "string", "description": "Id of the message this answers, like m123: threads it and quotes it"},
        "reply_all": {"type": "boolean", "description": "With reply_to: also to everyone the original went to"},
        "forward": {"type": "string", "description": "Id of the message to forward, with its attachments"},
        "account": {"type": "string", "description": "Account to write from (default: the replied or forwarded message's, otherwise the first)"}
    }) else {
        unreachable!()
    };
    map
}

pub fn send_schema() -> Value {
    let mut properties = compose_properties();
    properties.insert(
        "draft".into(),
        json!({"type": "string", "description": "Id of the draft this sends, like m123: deleted once sent. Pass the draft's content too (read it with mail_read)"}),
    );
    json!({"type": "object", "properties": properties, "required": ["body"]})
}

pub fn draft_schema() -> Value {
    let mut properties = compose_properties();
    properties.insert(
        "replace".into(),
        json!({"type": "string", "description": "Id of a draft this one replaces, like m123: deleted once this is saved"}),
    );
    json!({"type": "object", "properties": properties, "required": ["body"]})
}

/// The emails a call names: its `ids`, and those in its `ids_file`, each once.
fn parse_ids(args: &Value, workspace: &Path) -> Result<Vec<i64>, String> {
    const HELP: &str = "Name the emails: ids, a list of message ids like [\"m123\", \"m124\"], or ids_file, a \
        workspace file with them.";
    let mut given: Vec<Value> = match &args["ids"] {
        Value::Array(list) => list.clone(),
        Value::Null => Vec::new(),
        // A list sent as one string.
        Value::String(s) => ids_in_text(s)?,
        _ => return Err(HELP.into()),
    };
    if let Some(file) = arg_str(args, "ids_file") {
        let path = workspace_path(workspace, file)?;
        let text = std::fs::read_to_string(&path).map_err(|e| format!("Could not read {file}: {e}"))?;
        given.extend(ids_in_text(&text).map_err(|e| format!("In {file}: {e}"))?);
    }
    let mut ids: Vec<i64> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for v in &given {
        let id = match v {
            Value::String(raw) => message_id(raw)?,
            Value::Number(n) => n.as_i64().ok_or(HELP)?,
            _ => return Err(HELP.into()),
        };
        if seen.insert(id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err(HELP.into());
    }
    if ids.len() > MAX_IDS {
        return Err(format!(
            "That is {} emails; at most {MAX_IDS} go in one call: split them.",
            ids.len()
        ));
    }
    Ok(ids)
}

/// The ids in a file's text: JSON Lines with an `id` on each line (an export, filtered), a JSON
/// list of ids or of objects with one, or ids separated by spaces, commas or lines.
fn ids_in_text(text: &str) -> Result<Vec<Value>, String> {
    let id_of = |v: Value| match v {
        Value::Object(mut o) => o.remove("id").ok_or_else(|| "an entry has no \"id\".".to_string()),
        other => Ok(other),
    };
    let text = text.trim();
    if let Ok(Value::Array(list)) = serde_json::from_str::<Value>(text) {
        return list.into_iter().map(id_of).collect();
    }
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.first().is_some_and(|l| l.starts_with('{')) {
        return lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let v = serde_json::from_str::<Value>(line).map_err(|e| format!("line {} is not JSON: {e}.", i + 1))?;
                id_of(v)
            })
            .collect();
    }
    Ok(text
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '[' | ']'))
        .map(|t| t.trim_matches(|c| c == '"' || c == '\''))
        .filter(|t| !t.is_empty())
        .map(|t| Value::String(t.to_string()))
        .collect())
}

fn optional_id(args: &Value, key: &str) -> Result<Option<i64>, String> {
    arg_str(args, key).map(message_id).transpose()
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

fn cancelled(_: Cancelled) -> String {
    "Cancelled.".into()
}

/// The card that asks to change emails: what will happen and to which.
fn action_card(
    mail: &MailService,
    title: String,
    ids: &[i64],
    detail: Option<String>,
    confirm: &str,
    deny: &str,
) -> Value {
    let messages: Vec<Value> = ids
        .iter()
        .filter_map(|id| mail.summary(*id).ok())
        .take(SAMPLE)
        .map(|s| json!({"from": s.from.display(), "subject": s.subject}))
        .collect();
    json!({
        "kind": "mailAction",
        "title": title,
        "count": ids.len(),
        "messages": messages,
        "detail": detail,
        "confirm": confirm,
        "deny": deny,
    })
}

/// The result of a call the person declined.
fn declined(what: &str, note: &str) -> ToolOutput {
    ToolOutput::ok(
        json!({"status": "declined", "note": format!("The user chose not to {what}. {note}")}).to_string(),
        json!({"kind": "mailAction", "text": format!("Not allowed: {what}"), "denied": true}),
    )
}

/// The result: `text` for the model, `shown` for the person (without the model's next steps).
fn done(text: String, shown: String, stopped: Option<&str>) -> ToolOutput {
    ToolOutput::ok(text, json!({"kind": "mailAction", "text": shown, "stopped": stopped}))
}

/// What the report leaves to say: emails gone from the cache, and where it stopped.
fn report_tail(out: &mut String, missing: usize, stopped: Option<&str>) {
    if missing > 0 {
        out.push_str(&format!(
            " {} skipped: no longer where their ids were given, as they were moved or deleted since (by an \
             earlier call too). An email that moved has a new id in its new folder: list or search that folder \
             to work with it there.",
            plural(missing, "email")
        ));
    }
    if let Some(why) = stopped {
        out.push_str(&format!(
            " It stopped before the rest: {why}. Calling it again with the same ids (or ids_file) finishes the \
             job: those already done are no longer where they were and are skipped."
        ));
    }
}

/// The accounts the emails are in.
fn accounts_of(mail: &MailService, ids: &[i64]) -> Vec<Account> {
    let mut out: Vec<Account> = Vec::new();
    for id in ids {
        if let Ok(s) = mail.summary(*id)
            && !out.iter().any(|a| a.id == s.account)
            && let Ok(a) = mail.account(Some(&s.account))
        {
            out.push(a);
        }
    }
    out
}

pub async fn mark(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let ids = parse_ids(args, &ctx.workspace)?;
    let read = args["read"].as_bool();
    let starred = args["starred"].as_bool();
    if read.is_none() && starred.is_none() {
        return Err("Say what to mark: read (true or false), starred (true or false), or both.".into());
    }
    let mut changes = Vec::new();
    if let Some(r) = read {
        changes.push(if r { "read" } else { "unread" });
    }
    if let Some(s) = starred {
        changes.push(if s { "starred" } else { "not starred" });
    }
    let what = format!("mark {} as {}", plural(ids.len(), "email"), changes.join(" and "));
    let mail = &ctx.state.mail;
    let card = action_card(mail, capitalized(&what), &ids, None, "Mark", "Don't mark");
    if !ctx
        .confirm("mail_mark", Risk::Change, card, true)
        .await
        .map_err(cancelled)?
    {
        return Ok(declined(&what, "Do not retry; they stay as they are."));
    }
    let mut out = String::new();
    let mut shown = Vec::new();
    let mut stopped = None;
    for (bit, on, label) in [
        (SEEN, read, if read == Some(true) { "read" } else { "unread" }),
        (
            FLAGGED,
            starred,
            if starred == Some(true) { "starred" } else { "unstarred" },
        ),
    ] {
        let Some(on) = on else { continue };
        let r: Changed = mail.set_flag(&ids, bit, on).await.map_err(err)?;
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!("Marked {} {label}.", plural(r.changed, "email")));
        shown.push(format!("{} marked {label}", plural(r.changed, "email")));
        if r.already > 0 {
            out.push_str(&format!(
                " {} already {label}.",
                if r.already == 1 {
                    "1 was".to_string()
                } else {
                    format!("{} were", r.already)
                }
            ));
        }
        report_tail(&mut out, r.missing, r.stopped.as_deref());
        if r.stopped.is_some() {
            stopped = r.stopped;
            break;
        }
    }
    Ok(done(out, shown.join(" · "), stopped.as_deref()))
}

pub async fn move_to(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let ids = parse_ids(args, &ctx.workspace)?;
    let folder = require_str(args, "folder")?;
    let copy = args["copy"].as_bool().unwrap_or(false);
    let mail = &ctx.state.mail;
    // The destination is found in each account first, so a wrong name fails before asking.
    let mut names = Vec::new();
    for account in accounts_of(mail, &ids) {
        mail.folders(&account, false).await.map_err(err)?;
        let path = mail.destination(&account, folder).map_err(err)?;
        let name = mail
            .folders(&account, false)
            .await
            .ok()
            .and_then(|l| l.into_iter().find(|f| f.path == path).map(|f| f.name))
            .unwrap_or(path);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let to = if names.is_empty() {
        folder.to_string()
    } else {
        names.join(", ")
    };
    let verb = if copy { "copy" } else { "move" };
    let what = format!("{verb} {} to {to}", plural(ids.len(), "email"));
    let card = action_card(
        mail,
        capitalized(&what),
        &ids,
        None,
        if copy { "Copy" } else { "Move" },
        "Don't move",
    );
    if !ctx
        .confirm("mail_move", Risk::Change, card, true)
        .await
        .map_err(cancelled)?
    {
        return Ok(declined(&what, "Do not retry; they stay where they are."));
    }
    let r = mail.move_to(&ids, folder, copy).await.map_err(err)?;
    let mut out = format!(
        "{} {} to {to}.",
        if copy { "Copied" } else { "Moved" },
        plural(r.changed, "email")
    );
    if r.already > 0 {
        out.push_str(&format!(
            " {} already there.",
            if r.already == 1 {
                "1 was".to_string()
            } else {
                format!("{} were", r.already)
            }
        ));
    }
    let shown = out.clone();
    if r.changed > 0 && !copy {
        out.push_str(&format!(
            " In {to} they have new ids (list or search that folder to work with them there); every other email \
             keeps its id, so the ids you have for them, in exports too, still hold."
        ));
    } else if r.changed > 0 {
        out.push_str(&format!(
            " The copies in {to} have ids of their own; the emails keep theirs."
        ));
    }
    report_tail(&mut out, r.missing, r.stopped.as_deref());
    Ok(done(out, shown, r.stopped.as_deref()))
}

pub async fn delete(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let ids = parse_ids(args, &ctx.workspace)?;
    let permanent = args["permanent"].as_bool().unwrap_or(false);
    let mail = &ctx.state.mail;
    let for_good = mail.deletes_for_good(&ids, permanent).map_err(err)?;
    let n = plural(ids.len(), "email");
    let (what, card) = if for_good {
        let what = format!("delete {n} for good");
        let card = action_card(
            mail,
            capitalized(&what),
            &ids,
            Some("They cannot be recovered.".into()),
            "Delete for good",
            "Keep",
        );
        (what, card)
    } else {
        let what = format!("move {n} to the trash");
        let card = action_card(mail, capitalized(&what), &ids, None, "Delete", "Keep");
        (what, card)
    };
    let risk = if for_good { Risk::Risky } else { Risk::Change };
    if !ctx
        .confirm("mail_delete", risk, card, !for_good)
        .await
        .map_err(cancelled)?
    {
        return Ok(declined(&what, "Do not retry; they stay where they are."));
    }
    let r: Deleted = mail.delete(&ids, permanent).await.map_err(err)?;
    let mut parts = Vec::new();
    if r.trashed > 0 {
        parts.push(format!("Moved {} to the trash.", plural(r.trashed, "email")));
    }
    if r.destroyed > 0 {
        parts.push(format!("Deleted {} for good.", plural(r.destroyed, "email")));
    }
    if parts.is_empty() {
        parts.push("Deleted nothing.".into());
    }
    let shown = parts.join(" ");
    let mut out = shown.clone();
    report_tail(&mut out, r.missing, r.stopped.as_deref());
    Ok(done(out, shown, r.stopped.as_deref()))
}

pub async fn folder(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let action = require_str(args, "action")?;
    let mail = &ctx.state.mail;
    let account = mail.account(arg_str(args, "account")).map_err(err)?;
    let folders = mail.folders(&account, false).await.map_err(err)?;
    let gmail = account.kind == crate::mail::Kind::Gmail;
    let kind = if gmail { "label" } else { "folder" };
    let card = |title: String, detail: Option<String>, confirm: &str| json!({"kind": "mailAction", "title": title, "count": 0, "messages": [], "detail": detail, "confirm": confirm, "deny": "Cancel"});
    match action {
        "create" => {
            let name = require_str(args, "name")?;
            let parent = arg_str(args, "parent");
            let what = match parent {
                Some(p) => format!("create the {kind} {name} inside {p} in {}", account.label()),
                None => format!("create the {kind} {name} in {}", account.label()),
            };
            if !ctx
                .confirm(
                    "mail_folder",
                    Risk::Change,
                    card(capitalized(&what), None, "Create"),
                    true,
                )
                .await
                .map_err(cancelled)?
            {
                return Ok(declined(&what, "Do not retry."));
            }
            let made = crate::mail::folder_name(&mail.create_folder(&account, name, parent).await.map_err(err)?);
            let text = format!("Created the {kind} {made} in {}.", account.label());
            Ok(done(text.clone(), text, None))
        }
        "rename" => {
            let from = require_str(args, "folder")?;
            let to = require_str(args, "name")?;
            let what = format!("rename the {kind} {from} to {to}");
            if !ctx
                .confirm(
                    "mail_folder",
                    Risk::Change,
                    card(capitalized(&what), None, "Rename"),
                    true,
                )
                .await
                .map_err(cancelled)?
            {
                return Ok(declined(&what, "Do not retry."));
            }
            let renamed = crate::mail::folder_name(&mail.rename_folder(&account, from, to).await.map_err(err)?);
            let shown = format!("Renamed the {kind} {from} to {renamed}.");
            Ok(done(
                format!("{shown} Its emails have new ids: list it again to work with them."),
                shown,
                None,
            ))
        }
        "delete" => {
            let name = require_str(args, "folder")?;
            let path = mail.folder_path(&account, Some(name)).map_err(err)?;
            let total = folders.iter().find(|f| f.path == path).and_then(|f| f.total);
            let detail = if gmail {
                "The label goes; its emails stay in All Mail.".to_string()
            } else {
                match total {
                    Some(0) => "It is empty.".to_string(),
                    Some(n) => format!("Its {} are deleted with it, for good.", plural(n as usize, "email")),
                    None => "The emails in it are deleted with it, for good.".to_string(),
                }
            };
            let what = format!("delete the {kind} {name}");
            if !ctx
                .confirm(
                    "mail_folder",
                    Risk::Risky,
                    card(capitalized(&what), Some(detail), "Delete"),
                    false,
                )
                .await
                .map_err(cancelled)?
            {
                return Ok(declined(&what, "Do not retry."));
            }
            let deleted = mail.delete_folder(&account, name).await.map_err(err)?;
            let text = format!("Deleted the {kind} {deleted}.");
            Ok(done(text.clone(), text, None))
        }
        other => Err(format!("action must be create, rename or delete, not {other}.")),
    }
}

/// The account a message is written from: the one named, else the replied, forwarded or
/// drafted message's, else the first.
fn writing_account(mail: &MailService, args: &Value, from_message: Option<i64>) -> Result<Account, String> {
    if arg_str(args, "account").is_none()
        && let Some(id) = from_message
    {
        let s = mail.summary(id).map_err(err)?;
        return mail.account(Some(&s.account)).map_err(err);
    }
    mail.account(arg_str(args, "account")).map_err(err)
}

/// A list of addresses given as a list or as one string.
fn addresses(args: &Value, key: &str) -> Vec<String> {
    match &args[key] {
        Value::String(s) => vec![s.clone()],
        Value::Array(list) => list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

/// The workspace files to attach, read.
fn attachments(ctx: &ToolContext, args: &Value) -> Result<Vec<OutFile>, String> {
    let mut out = Vec::new();
    let mut total = 0u64;
    for rel in addresses(args, "attachments") {
        let path = workspace_path(&ctx.workspace, &rel)?;
        let bytes = std::fs::read(&path).map_err(|e| format!("could not read {rel}: {e}"))?;
        total += bytes.len() as u64;
        if total > MAX_ATTACHMENTS {
            return Err("The attachments come to more than 25 MB, which mail servers refuse.".into());
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| rel.clone());
        out.push(OutFile {
            mime: mime_of(&path).into(),
            name,
            bytes,
        });
    }
    Ok(out)
}

/// The message the call writes, put together.
async fn compose(
    ctx: &ToolContext,
    args: &Value,
    own_draft: Option<i64>,
) -> Result<(Account, Draft, Option<i64>), String> {
    let mail = &ctx.state.mail;
    let reply_to = optional_id(args, "reply_to")?;
    let forward = optional_id(args, "forward")?;
    let account = writing_account(mail, args, reply_to.or(forward).or(own_draft))?;
    let c = Compose {
        to: addresses(args, "to"),
        cc: addresses(args, "cc"),
        bcc: addresses(args, "bcc"),
        subject: arg_str(args, "subject").map(str::to_string),
        text: args["body"].as_str().unwrap_or_default().to_string(),
        attachments: attachments(ctx, args)?,
        reply_to,
        reply_all: args["reply_all"].as_bool().unwrap_or(false),
        forward,
        ..Compose::default()
    };
    let draft = mail.compose(&account, c).await.map_err(err)?;
    Ok((account, draft, reply_to))
}

fn send_card(draft: &Draft, saving: bool) -> Value {
    let list = |l: &[lettre::message::Mailbox]| l.iter().map(ToString::to_string).collect::<Vec<_>>();
    json!({
        "kind": "mailSend",
        "draft": saving,
        "from": draft.from.to_string(),
        "to": list(&draft.to),
        "cc": list(&draft.cc),
        "bcc": list(&draft.bcc),
        "subject": draft.subject,
        "body": draft.text,
        "attachments": draft.attachments.iter().map(|f| json!({"name": f.name, "size": f.bytes.len()})).collect::<Vec<_>>(),
    })
}

fn recipients(draft: &Draft) -> String {
    let all: Vec<String> = draft
        .to
        .iter()
        .chain(&draft.cc)
        .chain(&draft.bcc)
        .map(|m| m.email.to_string())
        .collect();
    match all.len() {
        0 => "nobody yet".into(),
        1..=4 => all.join(", "),
        n => format!("{} and {} others", all[..3].join(", "), n - 3),
    }
}

pub async fn send(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let from_draft = optional_id(args, "draft")?;
    let (account, draft, answers) = compose(ctx, args, from_draft).await?;
    if draft.to.is_empty() && draft.cc.is_empty() && draft.bcc.is_empty() {
        return Err("The email has nobody to go to: give to (or reply_to).".into());
    }
    let to = recipients(&draft);
    if !ctx
        .confirm("mail_send", Risk::Risky, send_card(&draft, false), false)
        .await
        .map_err(cancelled)?
    {
        return Ok(declined(
            &format!("send the email “{}” to {to}", draft.subject),
            "It was not sent. Ask them what to change; do not send it again unchanged.",
        ));
    }
    let notes = ctx
        .state
        .mail
        .send(&account, &draft, answers, from_draft)
        .await
        .map_err(err)?;
    let mut out = format!("Sent “{}” from {} to {to}.", draft.subject, account.email);
    if let Some(left) = &draft.left_out {
        out.push(' ');
        out.push_str(left);
    }
    for note in notes {
        out.push(' ');
        out.push_str(&note);
    }
    Ok(done(out.clone(), out, None))
}

pub async fn draft(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let replaces = optional_id(args, "replace")?;
    let (account, draft, _) = compose(ctx, args, replaces).await?;
    if !ctx
        .confirm("mail_draft", Risk::Change, send_card(&draft, true), true)
        .await
        .map_err(cancelled)?
    {
        return Ok(declined(
            &format!("save the draft “{}”", draft.subject),
            "Do not retry.",
        ));
    }
    let notes = ctx
        .state
        .mail
        .save_draft(&account, &draft, replaces)
        .await
        .map_err(err)?;
    let mut shown = format!(
        "Saved the draft “{}” to {} in the Drafts of {}.",
        draft.subject,
        recipients(&draft),
        account.email
    );
    if let Some(left) = &draft.left_out {
        shown.push(' ');
        shown.push_str(left);
    }
    for note in notes {
        shown.push(' ');
        shown.push_str(&note);
    }
    let out = format!("{shown} It has an id of its own in Drafts: list drafts to find it.");
    Ok(done(out, shown, None))
}

fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_read_once_each() {
        let here = Path::new(".");
        assert_eq!(
            parse_ids(&json!({"ids": ["m1", "m2", "m1", 3]}), here),
            Ok(vec![1, 2, 3])
        );
        assert!(parse_ids(&json!({"ids": []}), here).is_err());
        assert!(parse_ids(&json!({}), here).is_err());
        assert_eq!(parse_ids(&json!({"ids": "m1, m2"}), here), Ok(vec![1, 2]));
        assert!(parse_ids(&json!({"ids": "m1 mail"}), here).is_err());
    }

    #[test]
    fn ids_come_from_a_file_in_any_plain_shape() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, text: &str| std::fs::write(dir.path().join(name), text).unwrap();
        write(
            "keep.jsonl",
            "{\"id\": \"m7\", \"subject\": \"a\"}\n\n{\"id\": \"m9\", \"subject\": \"b\"}\n",
        );
        write("list.json", "[\"m3\", 4, {\"id\": \"m5\"}]");
        write("plain.txt", "m10 m11,m12\nm13");
        let ids = |file: &str| parse_ids(&json!({"ids_file": file}), dir.path());
        assert_eq!(ids("keep.jsonl"), Ok(vec![7, 9]));
        assert_eq!(ids("list.json"), Ok(vec![3, 4, 5]));
        assert_eq!(ids("plain.txt"), Ok(vec![10, 11, 12, 13]));
        // Both at once, each email once.
        assert_eq!(
            parse_ids(&json!({"ids": ["m1", "m7"], "ids_file": "keep.jsonl"}), dir.path()),
            Ok(vec![1, 7, 9])
        );
        assert!(ids("missing.txt").unwrap_err().contains("Could not read"));
        write("bad.jsonl", "{\"subject\": \"no id\"}");
        assert!(ids("bad.jsonl").is_err());
    }

    #[test]
    fn addresses_come_as_a_list_or_a_string() {
        assert_eq!(addresses(&json!({"to": "a@x.com"}), "to"), ["a@x.com"]);
        assert_eq!(addresses(&json!({"to": ["a@x.com", "b@x.com"]}), "to").len(), 2);
        assert!(addresses(&json!({}), "to").is_empty());
    }

    #[test]
    fn titles_start_with_a_capital() {
        assert_eq!(capitalized("move 2 emails to Archive"), "Move 2 emails to Archive");
        assert_eq!(plural(1, "email"), "1 email");
        assert_eq!(plural(2, "email"), "2 emails");
    }

    #[test]
    fn attachments_get_a_type() {
        assert_eq!(mime_of(Path::new("report.PDF")), "application/pdf");
        assert_eq!(mime_of(Path::new("data")), "application/octet-stream");
    }
}
