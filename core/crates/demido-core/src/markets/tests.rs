//! The feeds, against a stand-in on this computer that answers as Kraken, Coinbase and Frankfurter do.

use axum::extract::Query;
use axum::routing::get;
use serde_json::json;

use super::*;

async fn stand_in(kraken_fails: bool) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new()
        .route(
            "/0/public/OHLC",
            get(move |Query(q): Query<HashMap<String, String>>| async move {
                if kraken_fails {
                    return axum::Json(json!({ "error": ["EService:Unavailable"] }));
                }
                assert_eq!((q["pair"].as_str(), q["interval"].as_str()), ("XBTEUR", "1440"));
                axum::Json(json!({ "error": [], "result": { "XXBTZEUR": [[1795392000, "54875.9", "58984.3", "55799.1", "56128.1", "56500.0", "581.05", 1200], [1795478400, "56128.1", "56200.0", "55900.0", "56068.6", "56050.0", "0.37", 12]], "last": 1795392000 } }))
            }),
        )
        .route("/products/BTC-EUR/candles", get(|| async { axum::Json(json!([[1795478400, 55900.0, 56200.0, 56128.1, 56068.6, 0.37], [1795392000, 54875.9, 58984.3, 55799.1, 56128.1, 581.05]])) }))
        .route("/v1/{range}", get(|Query(q): Query<HashMap<String, String>>| async move { axum::Json(json!({ "base": q["base"], "rates": { "2026-11-23": { "USD": 1.0939 }, "2026-11-20": { "USD": 1.0901 } } })) }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    at
}

#[tokio::test]
async fn candles_come_from_kraken_from_coinbase_when_it_fails_and_are_kept_a_while() {
    let at = stand_in(false).await;
    let m = Markets::new(Feeds { kraken: at.clone(), coinbase: at.clone(), frankfurter: at.clone() }, std::sync::Arc::new(crate::db::Db::memory().unwrap()));
    let got = m.candles("BTC/EUR", 1440, false).await.unwrap();
    assert_eq!((got.from.as_str(), got.candles.len()), ("Kraken", 2));
    assert_eq!(got.candles[0], Candle { t: 1_795_392_000_000, o: 54875.9, h: 58984.3, l: 55799.1, c: 56128.1, v: 581.05 });
    let again = m.candles("BTC/EUR", 1440, false).await.unwrap();
    assert!(again.age_ms < 1000 && again.candles == got.candles, "kept, not asked again");

    let at = stand_in(true).await;
    let m = Markets::new(Feeds { kraken: at.clone(), coinbase: at.clone(), frankfurter: at.clone() }, std::sync::Arc::new(crate::db::Db::memory().unwrap()));
    let got = m.candles("BTC/EUR", 1440, false).await.unwrap();
    assert_eq!(got.from, "Coinbase");
    assert!(got.candles[0].t < got.candles[1].t, "oldest first, as every chart wants them");
    assert_eq!((got.candles[1].o, got.candles[1].h, got.candles[1].l), (56128.1, 56200.0, 55900.0), "Coinbase's own order of fields read right");

    let fx = m.candles("EUR/USD", 1440, false).await.unwrap();
    assert_eq!((fx.from.as_str(), fx.candles.len(), fx.candles[1].c), ("the European Central Bank", 2, 1.0939));
    assert!(m.candles("EUR/USD", 60, false).await.unwrap_err().to_string().contains("only daily candles"));
    assert!(m.candles("AAPL", 1440, false).await.is_err());
}

#[test]
fn the_script_in_an_answer_is_its_block_of_code_and_scripts_are_kept() {
    assert_eq!(script_in("Here it is:\n```pine\nindicator(\"x\")\nplot(close)\n```\nEnjoy."), "indicator(\"x\")\nplot(close)");
    assert_eq!(script_in("plot(close)"), "plot(close)");
    let asked = asked_to_write("an EMA of 10", Some("plot(ta.wma(close, 10))"), Some("line 1, column 6: ta.wma is not known"));
    assert!(asked.contains("The script so far:") && asked.contains("ta.wma is not known") && asked.ends_with("mended, in one block of code, and nothing else."));
    let db = crate::db::Db::memory().unwrap();
    let kept = keep(&db, None, "  My EMA ", "plot(close)").unwrap();
    assert_eq!(kept.name, "My EMA");
    keep(&db, Some(&kept.id), "My EMA", "plot(open)").unwrap();
    assert_eq!(scripts(&db).unwrap().iter().map(|s| s.source.as_str()).collect::<Vec<_>>(), ["plot(open)"]);
    forget(&db, &kept.id).unwrap();
    assert!(scripts(&db).unwrap().is_empty());
}
