#!/usr/bin/env python3
"""Stand-ins for Anthropic, Gemini and OpenRouter, and for the search services Brave, Exa and Tavily, on this
computer: to try Demido's providers and searches with a key without a key or money.

    python3 core/dev/providers-stand-in.py 8455 &
    DEMIDO_DEV_PROVIDERS=http://127.0.0.1:8455 DEMIDO_VAULT=file DEMIDO_HOME=/tmp/demido-trial \\
        core/target/debug/demido serve --listen 127.0.0.1:7399 --ui-dir apps/web/dist

Only a development build listens to DEMIDO_DEV_PROVIDERS. DEMIDO_VAULT=file keeps the trial's keys out of
the system keyring. Any key works here, and a key that starts with "bad" is refused, as a real provider
refuses one it does not know. Each answer says what it was asked, so a trial shows what Demido sent.

The shapes are the ones the providers' own documentation shows (docs/research/18-keys-and-providers.md, and
docs/research/30-search-with-a-key.md for the search services). Their results point at pages that do not
exist (example.org): a trial shows what was asked and what was counted, not real pages.
Nothing here reaches the internet.
"""
import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ANTHROPIC_MODELS = [
    {"id": "claude-opus-5-5", "display_name": "Claude Opus 5.5", "max_input_tokens": 1_000_000, "max_tokens": 128_000,
     "capabilities": {"image_input": {"supported": True}, "structured_outputs": {"supported": True},
                      "thinking": {"supported": True, "types": {"adaptive": {"supported": True}, "enabled": {"supported": False}}},
                      "effort": {"supported": True, "low": {"supported": True}, "medium": {"supported": True}, "high": {"supported": True}, "xhigh": {"supported": True}, "max": {"supported": True}}}},
    {"id": "claude-sonnet-5-5", "display_name": "Claude Sonnet 5.5", "max_input_tokens": 1_000_000, "max_tokens": 128_000,
     "capabilities": {"image_input": {"supported": True}, "structured_outputs": {"supported": True},
                      "thinking": {"supported": True, "types": {"adaptive": {"supported": True}}},
                      "effort": {"supported": True, "low": {"supported": True}, "medium": {"supported": True}, "high": {"supported": True}, "xhigh": {"supported": True}, "max": {"supported": True}}}},
    {"id": "claude-haiku-5-5", "display_name": "Claude Haiku 5.5", "max_input_tokens": 1_000_000, "max_tokens": 128_000,
     "capabilities": {"thinking": {"supported": True, "types": {"adaptive": {"supported": True}}},
                      "effort": {"supported": True, "low": {"supported": True}, "medium": {"supported": True}, "high": {"supported": True}, "xhigh": {"supported": True}, "max": {"supported": True}}}},
]
OPENROUTER_MODELS = [
    {"id": "meta-llama/llama-3.3-70b-instruct:free", "name": "Meta: Llama 3.3 70B Instruct (free)", "context_length": 131072,
     "pricing": {"prompt": "0", "completion": "0"}, "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]},
     "supported_parameters": ["max_tokens", "temperature", "response_format"]},
    {"id": "google/gemini-3.5-flash", "name": "Google: Gemini 3.5 Flash", "context_length": 1048576,
     "pricing": {"prompt": "0.0000015", "completion": "0.0000075"}, "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["text"]},
     "supported_parameters": ["reasoning", "response_format"], "reasoning": {"supported_efforts": ["high", "medium", "low", "minimal"], "mandatory": True}},
    {"id": "qwen/qwen3-235b-a22b", "name": "Qwen: Qwen3 235B A22B", "context_length": 131072,
     "pricing": {"prompt": "0.00000013", "completion": "0.0000006"}, "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]},
     "supported_parameters": ["reasoning", "response_format"]},
]
GEMINI_MODELS = ["gemini-3.8-flash", "gemini-3.5-flash-lite", "gemini-2.5-flash", "gemini-3.8-flash-tts", "gemini-embedding-2"]


def asked_about(body):
    """The last thing the person said, shortened: what the stand-in answers about."""
    last = next((m.get("content", "") for m in reversed(body.get("messages", [])) if m.get("role") == "user"), "")
    last = last if isinstance(last, str) else json.dumps(last)
    return last.strip().splitlines()[-1][:120] if last.strip() else "(nothing)"


def reply_for(body):
    """A decision when one is asked for (a JSON shape, or the tools' question), else an answer."""
    wants_json = "response_format" in body or "output_config" in body and "format" in body.get("output_config", {})
    last = json.dumps(body.get("messages", [])[-1:])
    if wants_json or "Reply with one JSON object" in last:
        # "search the web for <words>" in the person's message, before any result came back: a search, so that
        # a trial can see a search with a key go through a session. Once a result is in, no tool.
        said = json.dumps(body.get("messages", []))
        marker = "search the web for "
        if marker in said.lower() and "Results for" not in said and "No search service" not in said:
            at = said.lower().index(marker) + len(marker)
            words = said[at:].split("\\n")[0].split('"')[0].strip(" .?!")[:80]
            return json.dumps({"tool": "search_web", "args": {"query": words}})
        return '{"tool": "none", "args": {}}'
    return f"(Stand-in) You asked: “{asked_about(body)}”. This answer comes from core/dev/providers-stand-in.py, not from a real model."


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        sys.stderr.write("stand-in: " + fmt % args + "\n")

    def key(self):
        return self.headers.get("x-api-key") or (self.headers.get("authorization") or "").removeprefix("Bearer ").strip()

    def send_json(self, status, value):
        data = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def stream(self, events):
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("transfer-encoding", "chunked")
        self.end_headers()
        for kind, data in events:
            line = (f"event: {kind}\n" if kind else "") + f"data: {data if isinstance(data, str) else json.dumps(data)}\n\n"
            chunk = line.encode()
            self.wfile.write(f"{len(chunk):x}\r\n".encode() + chunk + b"\r\n")
            self.wfile.flush()
            time.sleep(0.03)
        self.wfile.write(b"0\r\n\r\n")

    def refuse_bad_key(self, provider):
        if not self.key().startswith("bad"):
            return False
        if provider == "anthropic":
            self.send_json(401, {"type": "error", "error": {"type": "authentication_error", "message": "invalid x-api-key"}})
        else:
            self.send_json(401, {"error": {"code": 401, "message": "No auth credentials found"}})
        return True

    def search_results(self, query):
        return [
            {"title": f"Stand-in result about {query}", "url": "https://example.org/stand-in-1", "passage": f"A stand-in passage that mentions {query}. Nothing here is real."},
            {"title": "A second stand-in page", "url": "https://example.org/stand-in-2", "passage": "Another passage, from a page that does not exist."},
        ]

    def do_GET(self):
        provider = self.path.strip("/").split("/")[0]
        if self.path.startswith("/brave/res/v1/llm/context"):
            if not self.headers.get("x-subscription-token") or self.headers.get("x-subscription-token").startswith("bad"):
                return self.send_json(401, {"type": "ErrorResponse", "error": {"code": "SUBSCRIPTION_TOKEN_INVALID", "status": 401}})
            from urllib.parse import parse_qs, urlparse
            query = parse_qs(urlparse(self.path).query).get("q", [""])[0]
            hits = self.search_results(query)
            return self.send_json(200, {"grounding": {"generic": [{"url": h["url"], "title": h["title"], "snippets": [h["passage"]]} for h in hits], "map": []},
                                        "sources": {h["url"]: {"title": h["title"], "hostname": "example.org", "age": ["Monday, November 16, 2026", "2026-11-16", "7 days ago", "2026-11-16T08:00:00Z"]} for h in hits}})
        if provider != "openrouter" and self.refuse_bad_key(provider):
            return
        if self.path.startswith("/anthropic/v1/models"):
            return self.send_json(200, {"data": ANTHROPIC_MODELS, "has_more": False, "last_id": ANTHROPIC_MODELS[-1]["id"]})
        if self.path == "/openrouter/models":
            return self.send_json(200, {"data": OPENROUTER_MODELS})
        if self.path == "/openrouter/key":
            if self.refuse_bad_key("openrouter"):
                return
            return self.send_json(200, {"data": {"label": "stand-in", "limit": 20.0, "limit_remaining": 18.75, "limit_reset": "monthly", "usage_monthly": 1.25,
                                                 "is_free_tier": False, "free_model_daily_requests": {"used": 3, "limit": 1000, "remaining": 997}}})
        if self.path == "/gemini/models":
            return self.send_json(200, {"object": "list", "data": [{"id": f"models/{m}", "object": "model", "owned_by": "google"} for m in GEMINI_MODELS]})
        self.send_json(404, {"error": {"message": f"nothing at {self.path}"}})

    def do_POST(self):
        provider = self.path.strip("/").split("/")[0]
        body = json.loads(self.rfile.read(int(self.headers.get("content-length") or 0)) or b"{}")
        if self.refuse_bad_key(provider):
            return
        if self.path == "/exa/search":
            hits = self.search_results(body.get("query", ""))
            return self.send_json(200, {"requestId": "stand-in", "results": [{"title": h["title"], "url": h["url"], "publishedDate": "2026-11-18T00:00:00.000Z", "highlights": [h["passage"]]} for h in hits],
                                        "costDollars": {"total": 0.004}, "searchTime": 100})
        if self.path == "/tavily/search":
            hits = self.search_results(body.get("query", ""))
            return self.send_json(200, {"query": body.get("query", ""), "answer": None, "images": [], "response_time": 0.5, "usage": {"credits": 1}, "request_id": "stand-in",
                                        "results": [{"title": h["title"], "url": h["url"], "content": h["passage"], "score": 0.8, "published_date": "Tue, 17 Nov 2026 17:00:00 GMT"} for h in hits]})
        text = reply_for(body)
        words = [w + " " for w in text.split(" ")]
        if self.path == "/anthropic/v1/messages":
            thinks = body.get("thinking", {}).get("type") in ("adaptive", "enabled")
            events = [("message_start", {"type": "message_start", "message": {"id": "msg_stand_in", "usage": {"input_tokens": 1200, "cache_creation_input_tokens": 300, "cache_read_input_tokens": 0, "output_tokens": 1}}})]
            if thinks:
                events += [("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}}),
                           ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "(Stand-in thinking.) "}})]
            events += [("content_block_delta", {"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": w}}) for w in words]
            events += [("message_delta", {"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 40 + len(words)}}), ("message_stop", {"type": "message_stop"})]
            return self.stream(events)
        if self.path in ("/openrouter/chat/completions", "/gemini/chat/completions"):
            free = str(body.get("model", "")).endswith(":free")
            cost = None if provider == "gemini" or free else round(0.00002 * (40 + len(words)), 6)
            events = [(None, {"id": "gen-stand-in", "choices": [{"index": 0, "delta": {"content": w}}]}) for w in words]
            events += [(None, {"id": "gen-stand-in", "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}),
                       (None, {"id": "gen-stand-in", "choices": [], "usage": {"prompt_tokens": 1500, "completion_tokens": 40 + len(words), "total_tokens": 1540 + len(words), "cost": cost, "prompt_tokens_details": {"cached_tokens": 0}}}),
                       (None, "[DONE]")]
            return self.stream(events)
        self.send_json(404, {"error": {"message": f"nothing at {self.path}"}})


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8455
    print(f"Provider stand-ins on http://127.0.0.1:{port}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
