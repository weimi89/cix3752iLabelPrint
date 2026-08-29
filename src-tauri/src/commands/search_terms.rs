//! 多組單號查詢的共用工具:把使用者貼上的一串單號拆成清單,並產生 `col IN (?, ?, …)` 子句。
//! 單號一律**精確比對**(不做 LIKE):條碼有固定格式,模糊比對只會把相似單號混進來。

/// 以逗號 / 分號 / 空白 / 換行 / Tab 切分,去頭尾空白、去重(保留順序)、略過空字串。
pub fn split_nos(raw: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in raw
        .unwrap_or("")
        .split(|c: char| c == ',' || c == ';' || c == '、' || c.is_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// `col IN (?, ?, …)`;n 必須 > 0(呼叫端先確認清單非空再組 WHERE)。
pub fn in_clause(col: &str, n: usize) -> String {
    debug_assert!(n > 0);
    let marks = std::iter::repeat("?").take(n).collect::<Vec<_>>().join(", ");
    format!("{col} IN ({marks})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_all_separators_and_dedups() {
        let v = split_nos(Some(" A1, B2;C3\nD4\tE5  F6、A1 ,, "));
        assert_eq!(v, vec!["A1", "B2", "C3", "D4", "E5", "F6"]);
        assert!(split_nos(Some("  ,\n ")).is_empty());
        assert!(split_nos(None).is_empty());
    }

    #[test]
    fn in_clause_shape() {
        assert_eq!(in_clause("query_no", 1), "query_no IN (?)");
        assert_eq!(in_clause("t", 3), "t IN (?, ?, ?)");
    }
}
