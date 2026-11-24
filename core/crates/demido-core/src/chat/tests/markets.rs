//! A model writes a Pine-compatible script, Demido runs it, hands back the error, and the model mends it (DECISIONS
//! M3, M18). Against a stand-in of Kraken's candles on this computer.

use axum::routing::get;

use super::*;

const NONE: &str = r#"{"tool": "none", "args": {}}"#;

#[tokio::test]
async fn a_script_the_engine_refuses_is_handed_back_with_its_error_and_mended() {
    let w = world("pine");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", listener.local_addr().unwrap());
    let rows: Vec<Value> = (0..40).map(|i| json!([1_795_000_000 + i * 86_400, "100", "102", "98", format!("{}", 100 + i % 5), "100", "1.0", 3])).collect();
    let app = axum::Router::new().route(
        "/0/public/OHLC",
        get(move || {
            let rows = rows.clone();
            async move { axum::Json(json!({ "error": [], "result": { "XXBTZEUR": rows, "last": 0 } })) }
        }),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let markets = crate::markets::Markets::new(crate::markets::Feeds { kraken: at.clone(), coinbase: at.clone(), frankfurter: at }, w.db.clone());
    // tools.use is off in this world: each try is one answer.
    let _ = NONE;
    w.stand.replies.lock().unwrap().extend([
        "```pine\nindicator(\"Fast\")\nplot(ta.wma(close, 10))\n```".to_string(),
        "Mended:\n```\n//@version=6\nindicator(\"Fast\", overlay=true)\nplot(ta.ema(close, 10), \"EMA 10\")\n```".to_string(),
    ]);
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let written = markets
        .write(&w.chat, &crate::markets::Write { request: "A fast moving average".into(), symbol: "BTC/EUR".into(), interval: 1440, source: None, session: Some(session.id.clone()), model: None })
        .await
        .unwrap();
    assert_eq!(written.tries, 2);
    assert!(written.error.is_none() && written.ran.as_ref().is_some_and(|r| r.overlay && r.plots[0].title == "EMA 10"), "{written:?}");
    assert!(written.source.starts_with("//@version=6"));
    // The second ask held the first script and the engine's words about it, for the model to mend.
    let messages = w.sessions.messages(&session.id).unwrap();
    let second = &messages[2].content;
    assert!(second.contains("plot(ta.wma(close, 10))") && second.contains("line 2, column 6: ta.wma is not known to Demido's engine"), "{second}");
    w.end().await;
}
