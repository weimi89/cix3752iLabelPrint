// 清關進度浮動框 store —— 全域(跨頁不消失)。預設追蹤「當日」報關進度,顯示 袋(剩/總)、件(剩/總);
// 日期區間另由設定對話框指定(預設當日)。即時更新全走雲端廣播(無輪詢):
//   - 已印  (clearance-date 頻道)→ applyPrinted:剩餘 -1(某袋印完則袋剩 -1)
//   - 新增  (報關日補上/改成本日期)→ applyAdded:件總/件剩 +,新袋則袋總/袋剩 +
//   - 移除  (報關日由本日期改走)→ applyRemoved:扣回對應總數/剩餘
// 另附「今日貼單單數(去重)」:雲端依業務日(06:00 起算)算 distinct 單數,與現場作業監控頁同口徑、
// 與報關日區間無關;區間內某件首次列印時本機 +1,區間外的件要等下次重抓(重整 / 重開 / 重連)才會反映。
// 內部以兩個 Map 維護(非響應式):_bagAll(每袋全部單號,算總數)、_bagUnprinted(每袋未印單號,算剩餘)。
import { defineStore } from 'pinia'
import { clearanceProgress, progressSetDates } from '@/api/tauri'

const POS_KEY = 'cix3752iLabelPrint.clearanceProgress.pos'
const loadPos = () => {
  try { const p = JSON.parse(localStorage.getItem(POS_KEY) || 'null'); if (p && Number.isFinite(p.x) && Number.isFinite(p.y)) return p } catch { /* ignore */ }
  return { x: 260, y: 90 }
}

const todayStr = () => {
  const d = new Date()
  const p = n => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

// 區間內每一天(含)→ ['YYYY-MM-DD', ...](i<366 僅為防呆上界,避免錯誤輸入造成無限/海量訂閱)
const datesInRange = (from, to) => {
  const out = []
  const start = new Date(from + 'T00:00:00')
  const end = new Date((to || from) + 'T00:00:00')
  for (let d = new Date(start), i = 0; d <= end && i < 366; d.setDate(d.getDate() + 1), i++) {
    const p = n => String(n).padStart(2, '0')
    out.push(`${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`)
  }
  return out
}

export const useClearanceProgress = defineStore('clearanceProgress', {
  state: () => ({
    open: false,
    from: todayStr(),
    to: todayStr(),
    bagTotal: 0,
    bagRemaining: 0,
    parcelTotal: 0,
    parcelRemaining: 0,
    stickerOrderNum: 0,   // 今日貼單單數(去重)
    stickerDate: '',      // 貼單數所屬業務日(Y-m-d)
    loading: false,
    error: '',
    loaded: false,
    pos: loadPos(),
    _bagAll: new Map(),       // package_sn → Set(全部 shipping_no)
    _bagUnprinted: new Map(), // package_sn → Set(未印 shipping_no)
    // 是否持有 parcels 明細:false = 雲端裁掉明細、退用總計數字(aggregate 模式)。
    // aggregate 模式下兩個 Map 為空,增量數學(applyPrinted/-Added/-Removed)必然與總計互踩
    //(剩餘凍結不遞減、_recount 把總數蓋成極小值)→ 一律改走「去抖重拉」。
    _detail: false,
    _refreshTimer: null,
  }),
  getters: {
    rangeLabel: s => (s.from === s.to ? s.from : `${s.from} ~ ${s.to}`),
  },
  actions: {
    openWidget() {
      this.open = true
      // 每次開啟都重抓並重新訂閱:close() 會退訂日期頻道、且資料可能已過時,
      // 若沿用舊 loaded 狀態會顯示舊數字又不即時遞減(已退訂沒重訂)。
      if (!this.loading) this.loadRange(this.from, this.to)
    },

    // 由兩個 Map 重算四個顯示數字
    _recount() {
      let pAll = 0; let pRemain = 0
      for (const set of this._bagAll.values()) pAll += set.size
      for (const set of this._bagUnprinted.values()) pRemain += set.size
      this.bagTotal = this._bagAll.size
      this.bagRemaining = this._bagUnprinted.size
      this.parcelTotal = pAll
      this.parcelRemaining = pRemain
    },

    async loadRange(from, to) {
      this.from = from
      this.to = to || from
      this.loading = true
      this.error = ''
      try {
        const r = await clearanceProgress(this.from, this.to)
        if (r.from) this.from = r.from
        if (r.to) this.to = r.to
        this.stickerOrderNum = Number(r.sticker?.order_num) || 0
        this.stickerDate = r.sticker?.business_date || ''
        const parcels = r.parcels || []
        const all = new Map()
        const unp = new Map()
        for (const p of parcels) {
          if (!all.has(p.package_sn)) all.set(p.package_sn, new Set())
          all.get(p.package_sn).add(p.shipping_no)
          if (p.printed) continue
          if (!unp.has(p.package_sn)) unp.set(p.package_sn, new Set())
          unp.get(p.package_sn).add(p.shipping_no)
        }
        this._bagAll = all
        this._bagUnprinted = unp
        this._detail = parcels.length > 0
        if (this._detail) {
          this._recount()
        } else {
          // 後端沒回 parcels 明細(例如裁切),退用雲端總計;
          // 此模式下 WS 事件改走 _scheduleAggregateRefresh 重拉(見 applyPrinted 等)
          this.bagTotal = r.bag_total || 0
          this.bagRemaining = r.bag_remaining || 0
          this.parcelTotal = r.parcel_total || 0
          this.parcelRemaining = r.remaining || 0
        }
        this.loaded = true
        await progressSetDates(datesInRange(this.from, this.to))
      } catch (e) {
        this.error = String(e?.message || e)
      } finally {
        this.loading = false
      }
    },

    // aggregate 模式(無明細)下的 WS 事件處理:無法做精準增量,改「去抖重拉」——
    // 2.5s 合併一次 loadRange 重抓雲端總計,零重複計數風險。
    // 計時器到期時若上一次 loadRange 還在跑(大區間查詢動輒數秒)→ **重排而非丟棄**:
    // 丟棄會讓「該袋最後一件」的更新永久遺失,剩餘數字凍結在 1 不歸零。
    _scheduleAggregateRefresh() {
      if (this._refreshTimer) return
      this._refreshTimer = setTimeout(() => {
        this._refreshTimer = null
        if (!this.open) return
        if (this.loading) { this._scheduleAggregateRefresh(); return }
        this.loadRange(this.from, this.to)
      }, 2500)
    },

    // WS 重連成功(sync-reconnected):斷線窗內的 clearance-date 廣播已遺失且無補洞,
    // 重拉基準校正(bag.* 另有後端 refresh_bag 補洞,不靠這裡)。
    // loadRange 進行中則排入去抖重拉,不丟棄(丟棄=斷線窗內的遺失永不校正)。
    reloadAfterReconnect() {
      if (!this.open || !this.loaded) return
      if (this.loading) { this._scheduleAggregateRefresh(); return }
      this.loadRange(this.from, this.to)
    },

    // 已印:件剩 -1(某袋最後一件印完 → 袋剩 -1);去重
    applyPrinted(shippingNo, packageSn) {
      if (!shippingNo || !this.loaded) return
      if (!this._detail) { this._scheduleAggregateRefresh(); return }
      let pkg = packageSn
      if (!pkg || !this._bagUnprinted.has(pkg)) {
        pkg = null
        for (const [k, set] of this._bagUnprinted) {
          if (set.has(shippingNo)) { pkg = k; break }
        }
      }
      if (!pkg) return
      const set = this._bagUnprinted.get(pkg)
      if (!set || !set.has(shippingNo)) return
      set.delete(shippingNo)
      if (this.parcelRemaining > 0) this.parcelRemaining--
      // 首次列印(仍在未印集合)才算新貼一單;補印同一件不重複計
      this.stickerOrderNum++
      if (set.size === 0) {
        this._bagUnprinted.delete(pkg)
        if (this.bagRemaining > 0) this.bagRemaining--
      }
    },

    // 新增(報關日補上/改入本區間):件總/件剩 +;新袋則袋總/袋剩 +;去重(已知件略過)
    applyAdded(parcels) {
      if (!this.loaded || !Array.isArray(parcels)) return
      if (!this._detail) { this._scheduleAggregateRefresh(); return }
      let changed = false
      for (const p of parcels) {
        const sn = p?.shipping_no; const pkg = p?.package_sn
        if (!sn || !pkg) continue
        if (this._bagAll.get(pkg)?.has(sn)) continue // 已知 → 去重
        if (!this._bagAll.has(pkg)) this._bagAll.set(pkg, new Set())
        this._bagAll.get(pkg).add(sn)
        // 已印件(報關日事後補上/改入區間時,該袋可能已印完)不計入未印,避免剩餘灌水
        if (!p.printed) {
          if (!this._bagUnprinted.has(pkg)) this._bagUnprinted.set(pkg, new Set())
          this._bagUnprinted.get(pkg).add(sn)
        }
        changed = true
      }
      if (changed) this._recount()
    },

    // 移除(報關日由本區間改走):扣回件;袋無剩餘件則袋總/袋剩同步收掉
    applyRemoved(parcels) {
      if (!this.loaded || !Array.isArray(parcels)) return
      if (!this._detail) { this._scheduleAggregateRefresh(); return }
      let changed = false
      for (const p of parcels) {
        const sn = p?.shipping_no; const pkg = p?.package_sn
        if (!sn || !pkg) continue
        const allSet = this._bagAll.get(pkg)
        if (!allSet || !allSet.has(sn)) continue // 不在追蹤中 → 略過
        allSet.delete(sn)
        if (allSet.size === 0) this._bagAll.delete(pkg)
        const unpSet = this._bagUnprinted.get(pkg)
        if (unpSet && unpSet.has(sn)) {
          unpSet.delete(sn)
          if (unpSet.size === 0) this._bagUnprinted.delete(pkg)
        }
        changed = true
      }
      if (changed) this._recount()
    },

    async close() {
      this.open = false
      if (this._refreshTimer) { clearTimeout(this._refreshTimer); this._refreshTimer = null }
      try { await progressSetDates([]) } catch { /* 退訂失敗略過 */ }
    },

    setPos(x, y) {
      this.pos = { x, y }
      try { localStorage.setItem(POS_KEY, JSON.stringify(this.pos)) } catch { /* ignore */ }
    },
  },
})
