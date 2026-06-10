/// 測試工具：產生所有錯誤面單樣本並存到 ~/Desktop/error_labels/
/// 執行：cd src-tauri && cargo run --bin gen_error_labels
fn main() {
    use barcoders::sym::code128::Code128;
    use cix3752i_label_print_lib::error_label::{generate, LabelHeight};

    // 診斷 Code128
    match Code128::new("TW12345678901".to_string()) {
        Ok(bc) => println!("[barcode] Code128 OK: {} modules", bc.encode().len()),
        Err(e) => eprintln!("[barcode] Code128 失敗: {e:?}"),
    }

    let codes = [
        ("STORE_CLOSED", "門市關轉"),
        ("UNCONFIRMED",  "訂單未確認"),
        ("NOT_FOUND",    "找不到包裹"),
        ("NOT_PROXY",    "非代寄訂單"),
        ("NOT_FORWARD",  "非轉寄訂單"),
        ("ABNORMAL",     "包裹異常"),
        ("ERROR",        "面單請求失敗"),
    ];

    let query_no = "TW12345678901";

    let out_dir = std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join("Desktop").join("error_labels"))
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp/error_labels"));

    std::fs::create_dir_all(&out_dir).expect("無法建立輸出目錄");

    for (code, label) in &codes {
        let bytes = generate(query_no, code, LabelHeight::H100mm);
        let fname = format!("{code}.png");
        let path = out_dir.join(&fname);
        std::fs::write(&path, &bytes).expect("寫檔失敗");
        println!("✓  {label} → {}", path.display());
    }

    println!("\n所有樣本已存到: {}", out_dir.display());
}
