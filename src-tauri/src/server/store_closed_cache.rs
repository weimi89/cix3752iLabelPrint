//! 門市關轉單號的短期記憶。
//!
//! 現場一件關轉包裹會被一投再投（2026-09-18 同一件走了 4 趟異常口，每趟中介機都去雲端問一次、
//! 每趟都回一樣的「門市關轉」）。同一單號在 [`TTL`] 內再被查到，直接回上次雲端給的錯誤，
//! 不再打雲端：工控機更快拿到結果、雲端少挨無意義的查詢。其他業務錯誤（未確認、查無訂單）
//! 客服處理完就會變，不能記。

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 記多久：關轉的門市不會在幾小時內重開，但也不要記到隔天
pub const TTL: Duration = Duration::from_secs(2 * 60 * 60);

/// 上次雲端回的關轉錯誤，重播時原樣還原成 `AppError::Cloud`
#[derive(Debug, Clone, PartialEq)]
pub struct CachedCloudError {
    pub code: String,
    pub message: String,
    pub shipping_provider: Option<String>,
    pub shipping_no: Option<String>,
    pub package_sn: Option<String>,
    pub order_sn: Option<String>,
}

#[derive(Default)]
pub struct StoreClosedCache {
    map: HashMap<String, (CachedCloudError, Instant)>,
}

impl StoreClosedCache {
    /// 只記門市關轉；其他錯誤碼呼叫也不會記
    pub fn remember(&mut self, query_no: &str, err: CachedCloudError, now: Instant) {
        if err.code != "STORE_CLOSED" {
            return;
        }
        self.map.insert(query_no.to_string(), (err, now));
    }

    /// 命中就回上次的錯誤；過期的順手清掉
    pub fn lookup(&mut self, query_no: &str, now: Instant) -> Option<CachedCloudError> {
        match self.map.get(query_no) {
            Some((err, at)) if now.duration_since(*at) < TTL => Some(err.clone()),
            Some(_) => {
                self.map.remove(query_no);
                None
            }
            None => None,
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed() -> CachedCloudError {
        CachedCloudError {
            code: "STORE_CLOSED".into(),
            message: "無法列印，訂單門市關轉".into(),
            shipping_provider: Some("SEVEN".into()),
            shipping_no: Some("74Z01396865".into()),
            package_sn: Some("PKG1".into()),
            order_sn: None,
        }
    }

    #[test]
    fn 關轉單號兩小時內再查直接命中() {
        let mut c = StoreClosedCache::default();
        let t0 = Instant::now();
        c.remember("74Z01396865", closed(), t0);
        assert_eq!(c.lookup("74Z01396865", t0 + Duration::from_secs(60)), Some(closed()));
        assert_eq!(c.lookup("74Z01396865", t0 + TTL - Duration::from_secs(1)), Some(closed()));
    }

    #[test]
    fn 過期就不命中並清掉() {
        let mut c = StoreClosedCache::default();
        let t0 = Instant::now();
        c.remember("A", closed(), t0);
        assert_eq!(c.lookup("A", t0 + TTL), None);
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn 其他錯誤碼不記_別的單號不命中() {
        let mut c = StoreClosedCache::default();
        let t0 = Instant::now();
        c.remember("B", CachedCloudError { code: "UNCONFIRMED".into(), ..closed() }, t0);
        assert_eq!(c.lookup("B", t0), None);
        c.remember("A", closed(), t0);
        assert_eq!(c.lookup("C", t0), None);
    }
}
