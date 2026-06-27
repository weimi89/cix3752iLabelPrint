// 清關進度浮動框 store —— 全域(跨頁不消失)。預設追蹤「當日」報關進度,顯示 袋(剩/總)、件(剩/總);
// 日期區間另由設定對話框指定(預設當日)。即時更新全走雲端廣播(無輪詢):
//   - 已印  (clearance-date 頻道)→ applyPrinted:剩餘 -1(某袋印完則袋剩 -1)
//   - 新增  (報關日補上/改成本日期)→ applyAdded:件總/件剩 +,新袋則袋總/袋剩 +
//   - 移除  (報關日由本日期改走)→ applyRemoved:扣回對應總數/剩餘
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
    loading: false,
    error: '',
    loaded: false,
    pos: loadPos(),
    _bagAll: new Map(),       // package_sn → Set(全部 shipping_no)
    _bagUnprinted: new Map(), // package_sn → Set(未印 shipping_no)
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
        if (parcels.length) {
          this._recount()
        } else {
          // 後端沒回 parcels 明細(例如裁切),退用雲端總計
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

    // 已印:件剩 -1(某袋最後一件印完 → 袋剩 -1);去重
    applyPrinted(shippingNo, packageSn) {
      if (!shippingNo || !this.loaded) return
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
      if (set.size === 0) {
        this._bagUnprinted.delete(pkg)
        if (this.bagRemaining > 0) this.bagRemaining--
      }
    },

    // 新增(報關日補上/改入本區間):件總/件剩 +;新袋則袋總/袋剩 +;去重(已知件略過)
    applyAdded(parcels) {
      if (!this.loaded || !Array.isArray(parcels)) return
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
      try { await progressSetDates([]) } catch { /* 退訂失敗略過 */ }
    },

    setPos(x, y) {
      this.pos = { x, y }
      try { localStorage.setItem(POS_KEY, JSON.stringify(this.pos)) } catch { /* ignore */ }
    },
  },
})
