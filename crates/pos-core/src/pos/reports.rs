use super::catalog::{product_row, PRODUCT_COLS, PRODUCT_FROM};
use super::sales::{sale_row, SALE_COLS, SALE_FROM};
use super::Pos;
use crate::error::Result;
use crate::models::*;
use crate::money::Money;
use crate::perms::Perm;
use crate::util::{add_days, day_after, day_start, month_start, parse_date, today};
use rusqlite::params;

impl Pos {
    fn sales_and_profit(&self, from_ts: &str, to_ts: &str) -> Result<(Money, Money, i64)> {
        let (total, count): (Money, i64) = self.conn.query_row(
            "SELECT COALESCE(SUM(total), 0), COALESCE(SUM(CASE WHEN kind <> 'return' THEN 1 ELSE 0 END), 0)
             FROM sales WHERE status = 'completed' AND created_at >= ?1 AND created_at < ?2",
            params![from_ts, to_ts],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let profit: Money = self.conn.query_row(
            "SELECT COALESCE(SUM(si.net_amount - si.cost_amount), 0) FROM sale_items si
             JOIN sales s ON s.id = si.sale_id
             WHERE s.status = 'completed' AND s.created_at >= ?1 AND s.created_at < ?2",
            params![from_ts, to_ts],
            |r| r.get(0),
        )?;
        Ok((total, profit, count))
    }

    fn by_day(&self, from: &str, to: &str) -> Result<Vec<DayTotal>> {
        let mut st = self.conn.prepare(
            "SELECT substr(s.created_at, 1, 10) AS d, SUM(s.total),
                SUM((SELECT COALESCE(SUM(net_amount - cost_amount), 0) FROM sale_items WHERE sale_id = s.id)),
                SUM(CASE WHEN s.kind <> 'return' THEN 1 ELSE 0 END)
             FROM sales s WHERE s.status = 'completed' AND s.created_at >= ?1 AND s.created_at < ?2
             GROUP BY d",
        )?;
        let found: Vec<(String, Money, Money, i64)> = st
            .query_map(params![day_start(from), day_after(to)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?;
        // Fill every day so charts have no gaps (max ~400 days).
        let mut out = Vec::new();
        let (Some(start), Some(end)) = (parse_date(from), parse_date(to)) else { return Ok(out) };
        let mut d = start;
        let mut guard = 0;
        while d <= end && guard < 400 {
            let ds = d.format("%Y-%m-%d").to_string();
            let hit = found.iter().find(|f| f.0 == ds);
            out.push(DayTotal {
                label: d.format("%a %-d").to_string(),
                date: ds,
                total: hit.map(|h| h.1).unwrap_or(0),
                profit: hit.map(|h| h.2).unwrap_or(0),
                count: hit.map(|h| h.3).unwrap_or(0),
            });
            d = d.succ_opt().unwrap_or(d);
            guard += 1;
        }
        Ok(out)
    }

    pub fn dashboard(&self) -> Result<Dashboard> {
        self.require(Perm::Dashboard)?;
        let t = today();
        let (from, to) = (day_start(&t), day_after(&t));
        let (sales_today, profit_today, count_today) = self.sales_and_profit(&from, &to)?;
        let cash_today = self.payments_between(&from, &to, PayMethod::Cash)?;
        let bank_today = self.payments_between(&from, &to, PayMethod::Bank)?;
        let ms = month_start(&t);
        let (sales_month, profit_month, _) = self.sales_and_profit(&day_start(&ms), &to)?;
        let one = |sql: &str| -> Result<i64> { Ok(self.conn.query_row(sql, [], |r| r.get(0))?) };
        let vehicles_in_stock = one("SELECT COUNT(*) FROM vehicles WHERE status IN ('in_stock','demo')")?;
        let vehicles_sold_month: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM vehicles WHERE status = 'sold' AND sold_at >= ?1",
            params![ms],
            |r| r.get(0),
        )?;
        let open_jobs = one("SELECT COUNT(*) FROM job_cards WHERE status NOT IN ('delivered','cancelled')")?;
        let ready_jobs = one("SELECT COUNT(*) FROM job_cards WHERE status = 'ready'")?;
        let low_stock_count = one("SELECT COUNT(*) FROM products WHERE active = 1 AND stock > 0 AND stock <= low_stock")?;
        let out_of_stock_count = one("SELECT COUNT(*) FROM products WHERE active = 1 AND stock <= 0")?;
        let open_bookings = one("SELECT COUNT(*) FROM bookings WHERE status = 'open'")?;
        let receivables: Money = self.conn.query_row(
            "SELECT COALESCE(SUM(s.total - (SELECT COALESCE(SUM(amount), 0) FROM payments WHERE sale_id = s.id)), 0)
             FROM sales s WHERE s.status = 'completed' AND s.customer_id IS NOT NULL AND s.total > 0",
            [],
            |r| r.get(0),
        )?;
        let last7 = self.by_day(&add_days(&t, -6), &t)?;
        let mut st = self.conn.prepare(&format!("SELECT {SALE_COLS} FROM {SALE_FROM} ORDER BY s.id DESC LIMIT 8"))?;
        let recent_sales = st.query_map([], sale_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut st = self.conn.prepare(&format!(
            "SELECT {PRODUCT_COLS} FROM {PRODUCT_FROM} WHERE p.active = 1 AND p.stock <= p.low_stock
             ORDER BY p.stock <= 0 DESC, (p.stock * 1.0 / MAX(p.low_stock, 1)), p.name LIMIT 8"
        ))?;
        let low_stock = st.query_map([], product_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut jobs_by_status = Vec::new();
        for s in JobStatus::BOARD {
            let n: i64 =
                self.conn.query_row("SELECT COUNT(*) FROM job_cards WHERE status = ?1", params![s], |r| r.get(0))?;
            jobs_by_status.push((s, n));
        }
        let service_due = self.service_due(7)?;
        Ok(Dashboard {
            sales_today,
            count_today,
            profit_today,
            cash_today,
            bank_today,
            sales_month,
            profit_month,
            vehicles_in_stock,
            vehicles_sold_month,
            open_jobs,
            ready_jobs,
            low_stock_count,
            out_of_stock_count,
            receivables,
            open_bookings,
            last7,
            recent_sales,
            low_stock,
            jobs_by_status,
            service_due,
        })
    }

    /// Profit sheet for a date range (inclusive), built from every sale's cost and price.
    pub fn profit_sheet(&self, from: &str, to: &str) -> Result<ProfitSheet> {
        self.require(Perm::Reports)?;
        self.require(Perm::ViewCost)?;
        let (f, t) = (day_start(from), day_after(to));
        let mut st = self.conn.prepare(
            "SELECT si.section, si.group_key, MAX(si.item_name), MAX(si.code), SUM(si.qty), SUM(si.net_amount),
                SUM(si.cost_amount)
             FROM sale_items si JOIN sales s ON s.id = si.sale_id
             WHERE s.status = 'completed' AND s.created_at >= ?1 AND s.created_at < ?2
             GROUP BY si.section, si.group_key
             ORDER BY SUM(si.net_amount - si.cost_amount) DESC",
        )?;
        let items: Vec<ProfitItem> = st
            .query_map(params![f, t], |r| {
                let section: Section = r.get(0)?;
                let group_key: String = r.get(1)?;
                let revenue: Money = r.get(5)?;
                let cost: Money = r.get(6)?;
                let code: String = r.get(3)?;
                Ok(ProfitItem {
                    section,
                    code: if group_key.starts_with("m:") { String::new() } else { code },
                    name: r.get(2)?,
                    qty: r.get(4)?,
                    revenue,
                    cost,
                    profit: revenue - cost,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut sections = Vec::new();
        for sec in [Section::Vehicle, Section::Part, Section::Labor, Section::WorkshopParts, Section::Other] {
            let mine: Vec<&ProfitItem> = items.iter().filter(|i| i.section == sec).collect();
            if mine.is_empty() && sec == Section::Other {
                continue;
            }
            let revenue: Money = mine.iter().map(|i| i.revenue).sum();
            let cost: Money = mine.iter().map(|i| i.cost).sum();
            sections.push(ProfitSection {
                section: sec,
                qty: mine.iter().map(|i| i.qty).sum(),
                revenue,
                cost,
                profit: revenue - cost,
            });
        }
        let revenue: Money = items.iter().map(|i| i.revenue).sum();
        let cost: Money = items.iter().map(|i| i.cost).sum();
        let (discounts, tax, sales_count, returns): (Money, Money, i64, Money) = self.conn.query_row(
            "SELECT COALESCE(SUM(discount), 0) + COALESCE((SELECT SUM(si.line_discount) FROM sale_items si
                    JOIN sales s2 ON s2.id = si.sale_id WHERE s2.status = 'completed' AND s2.kind <> 'return'
                    AND s2.created_at >= ?1 AND s2.created_at < ?2), 0),
                COALESCE(SUM(tax), 0),
                COALESCE(SUM(CASE WHEN kind <> 'return' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN kind = 'return' THEN -total ELSE 0 END), 0)
             FROM sales WHERE status = 'completed' AND created_at >= ?1 AND created_at < ?2",
            params![f, t],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let warranty_cost: Money = self.conn.query_row(
            "SELECT COALESCE(SUM(si.cost_amount), 0) FROM sale_items si JOIN sales s ON s.id = si.sale_id
             WHERE s.status = 'completed' AND si.free = 1 AND s.created_at >= ?1 AND s.created_at < ?2",
            params![f, t],
            |r| r.get(0),
        )?;
        let by_day = self.by_day(from, to)?;
        Ok(ProfitSheet {
            from: from.into(),
            to: to.into(),
            revenue,
            cost,
            profit: revenue - cost,
            discounts,
            tax,
            warranty_cost,
            sales_count,
            returns,
            sections,
            items,
            by_day,
        })
    }

    pub fn sales_summary(&self, from: &str, to: &str) -> Result<SalesSummary> {
        self.require(Perm::Reports)?;
        let (f, t) = (day_start(from), day_after(to));
        let (total, count): (Money, i64) = self.conn.query_row(
            "SELECT COALESCE(SUM(total), 0), COALESCE(SUM(CASE WHEN kind <> 'return' THEN 1 ELSE 0 END), 0)
             FROM sales WHERE status = 'completed' AND created_at >= ?1 AND created_at < ?2",
            params![f, t],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut by_method = Vec::new();
        for m in PayMethod::ALL {
            by_method.push((*m, self.payments_between(&f, &t, *m)?));
        }
        let mut by_kind = Vec::new();
        for k in SaleKind::ALL {
            let (n, amt): (i64, Money) = self.conn.query_row(
                "SELECT COUNT(*), COALESCE(SUM(total), 0) FROM sales
                 WHERE status = 'completed' AND kind = ?1 AND created_at >= ?2 AND created_at < ?3",
                params![k, f, t],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            by_kind.push((*k, n, amt));
        }
        let mut st = self.conn.prepare(
            "SELECT COALESCE(u.name, '—'), COUNT(*), COALESCE(SUM(s.total), 0)
             FROM sales s LEFT JOIN users u ON u.id = s.salesperson_id
             WHERE s.status = 'completed' AND s.kind <> 'return' AND s.created_at >= ?1 AND s.created_at < ?2
             GROUP BY s.salesperson_id ORDER BY 3 DESC",
        )?;
        let by_staff = st
            .query_map(params![f, t], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(SalesSummary { total, count, by_method, by_kind, by_staff })
    }

    pub fn stock_report(&self) -> Result<StockReport> {
        self.require(Perm::Products)?;
        let r = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(MAX(stock, 0)), 0), COALESCE(SUM(MAX(stock, 0) * cost), 0),
                COALESCE(SUM(MAX(stock, 0) * price), 0),
                COALESCE(SUM(CASE WHEN stock > 0 AND stock <= low_stock THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN stock <= 0 THEN 1 ELSE 0 END), 0)
             FROM products WHERE active = 1",
            [],
            |r| {
                Ok(StockReport {
                    products: r.get(0)?,
                    units: r.get(1)?,
                    value_cost: r.get(2)?,
                    value_retail: r.get(3)?,
                    low: r.get(4)?,
                    out: r.get(5)?,
                    vehicles: 0,
                    vehicles_value: 0,
                })
            },
        )?;
        let (vehicles, vehicles_value): (i64, Money) = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(cost), 0) FROM vehicles WHERE status IN ('in_stock','demo','booked','in_transit')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(StockReport { vehicles, vehicles_value, ..r })
    }
}
