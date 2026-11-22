//! `demido pdf-text`, as the server uses it: a PDF's text taken out in a process of its own (web/pdf.rs).

use demido_core::web::pdf::{Reader, read};

/// A one-page PDF that says "Hello from a PDF".
fn hello() -> Vec<u8> {
    let stream = "BT /F1 18 Tf 36 72 Td (Hello from a PDF) Tj ET";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 144] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut starts = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        starts.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let table = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for start in starts {
        out.extend_from_slice(format!("{start:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

#[tokio::test]
async fn a_pdf_is_read_apart_from_the_server_and_a_broken_one_ends_only_that_process() {
    let apart = Reader::Apart(env!("CARGO_BIN_EXE_demido").into());
    let read_it = read(hello(), &apart).await.unwrap();
    assert!(read_it.contains("Hello from a PDF"), "{read_it}");
    let said = read(b"%PDF-1.7 not really".to_vec(), &apart).await.unwrap_err();
    assert!(said.starts_with("the PDF could not be read"), "{said}");
}
