import { useEffect, useState, useMemo } from "react";
import { type PersonRecord } from "@/App";
import { apiGet, apiPost, apiDel } from "@/lib/api";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent } from "@/components/ui/card";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter,
} from "@/components/ui/dialog";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { formatINR, formatDate, rupeesToPaise, dateToFY } from "@/lib/format";
import { cn } from "@/lib/utils";

interface Portfolio { portfolio_id: number; person_id: number | null; name: string; }
interface Account   { account_id: number; portfolio_id: number; name: string; }

interface TaxEntry {
  entry_id:     number;
  person_id:    number;
  person_name:  string;
  entry_type:   string;
  amount_paise: number;
  entry_date:   string;
  fy:           string;
  txn_id?:      number;
  notes?:       string;
  created_at:   string;
}

interface CapitalGainsSummary {
  fy: string;
  stcg_equity_paise: number;
  ltcg_equity_paise: number;
  stcg_debt_paise: number;
  ltcg_debt_paise: number;
  speculative_paise: number;
  non_speculative_paise: number;
  total_gain_paise: number;
}

const ENTRY_TYPE_LABELS: Record<string, string> = {
  TDS:                  "TDS",
  ADVANCE_TAX:          "Advance Tax",
  SELF_ASSESSMENT_TAX:  "Self Assessment Tax",
};

const ENTRY_TYPE_COLORS: Record<string, string> = {
  TDS:                 "bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-400",
  ADVANCE_TAX:         "bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400",
  SELF_ASSESSMENT_TAX: "bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400",
};

const LTCG_EQUITY_EXEMPTION_PAISE = 125_000_00;

function computeTax(summary: CapitalGainsSummary) {
  const stcg_tax = Math.max(0, summary.stcg_equity_paise) * 0.20;
  const ltcg_taxable = Math.max(0, summary.ltcg_equity_paise - LTCG_EQUITY_EXEMPTION_PAISE);
  const ltcg_equity_tax = ltcg_taxable * 0.125;
  const ltcg_debt_tax = Math.max(0, summary.ltcg_debt_paise) * 0.125;
  const total = stcg_tax + ltcg_equity_tax + ltcg_debt_tax;
  return {
    stcg_tax: Math.round(stcg_tax),
    ltcg_equity_tax: Math.round(ltcg_equity_tax),
    ltcg_debt_tax: Math.round(ltcg_debt_tax),
    total: Math.round(total),
  };
}

const today = new Date().toISOString().split("T")[0];
const currentFY = dateToFY(today);

export function TaxPage({ activePerson, personAccountIds: personAccountIdsProp }: {
  activePerson?: PersonRecord | null;
  personAccountIds?: number[] | null;
}) {
  // ── Portfolio / account filter (scoped to this person) ───────────────────
  const [portfolios,  setPortfolios]  = useState<Portfolio[]>([]);
  const [allAccounts, setAllAccounts] = useState<Account[]>([]);
  const [selectedPortfolioId, setSelectedPortfolioId] = useState<string>("all");
  const [selectedAccountIds,  setSelectedAccountIds]  = useState<number[]>([]);

  // ── FY selector ───────────────────────────────────────────────────────────
  const [fy, setFY] = useState<string>(currentFY);

  // ── Capital gains data ────────────────────────────────────────────────────
  const [cgSummary, setCgSummary] = useState<CapitalGainsSummary | null>(null);
  const [allCgFYs,  setAllCgFYs]  = useState<string[]>([]);
  const [cgLoading, setCgLoading] = useState(false);

  // ── Tax ledger ─────────────────────────────────────────────────────────────
  const [entries, setEntries] = useState<TaxEntry[]>([]);

  // ── Add dialog ─────────────────────────────────────────────────────────────
  const [showAdd,  setShowAdd]  = useState(false);
  const [delEntry, setDelEntry] = useState<TaxEntry | null>(null);
  const [saving,   setSaving]   = useState(false);
  const [error,    setError]    = useState("");

  const [fEntryType, setFEntryType] = useState("TDS");
  const [fAmount,    setFAmount]    = useState("");
  const [fDate,      setFDate]      = useState(today);
  const [fFY,        setFFY]        = useState(dateToFY(today));
  const [fNotes,     setFNotes]     = useState("");

  // ── Load portfolios + accounts (scoped to person via personAccountIdsProp) ─
  useEffect(() => {
    if (personAccountIdsProp === undefined) return;
    // Reset immediately so stale accounts from previous person don't leak
    setPortfolios([]);
    setAllAccounts([]);
    setSelectedAccountIds([]);
    if (personAccountIdsProp[0] === -1) return; // person has no accounts
    Promise.all([
      apiGet<Portfolio[]>("/portfolios"),
      apiGet<Account[]>("/accounts"),
    ]).then(([ps, as_]) => {
      const scopedAccIds = new Set(personAccountIdsProp);
      const filteredAs = as_.filter(a => scopedAccIds.has(a.account_id));
      const portfolioIds = new Set(filteredAs.map(a => a.portfolio_id));
      const filteredPs = ps.filter(p => portfolioIds.has(p.portfolio_id));
      setPortfolios(filteredPs);
      setAllAccounts(filteredAs);
      setSelectedAccountIds(filteredAs.map(a => a.account_id));
    }).catch(() => {});
  }, [personAccountIdsProp]);

  // Accounts visible under selected portfolio filter
  const visibleAccounts = useMemo(() =>
    selectedPortfolioId === "all"
      ? allAccounts
      : allAccounts.filter(a => a.portfolio_id === parseInt(selectedPortfolioId)),
    [allAccounts, selectedPortfolioId]
  );

  // When portfolio filter changes, reset account selection to all visible
  useEffect(() => {
    setSelectedAccountIds(visibleAccounts.map(a => a.account_id));
  }, [selectedPortfolioId]);

  // Effective account IDs for fetches — personAccountIdsProp as absolute baseline
  const effectiveAccountIds = useMemo((): number[] | null => {
    if (selectedAccountIds.length > 0) return selectedAccountIds;
    if (personAccountIdsProp && personAccountIdsProp[0] !== -1) return personAccountIdsProp;
    return [-1];
  }, [selectedAccountIds, personAccountIdsProp]);

  // ── Load capital gains ─────────────────────────────────────────────────────
  useEffect(() => {
    if (personAccountIdsProp === undefined) return;
    setCgLoading(true);
    apiPost<{ summaries: CapitalGainsSummary[]; all_fys: string[] }>(
      "/reports/capital-gains",
      { fy: null, account_ids: effectiveAccountIds },
    ).then(data => {
      setAllCgFYs(data.all_fys);
      const match = data.summaries.find(s => s.fy === fy) ?? null;
      setCgSummary(match);
    }).catch(() => {
      setCgSummary(null);
    }).finally(() => setCgLoading(false));
  }, [effectiveAccountIds, fy, personAccountIdsProp]);

  // ── Load tax ledger entries ────────────────────────────────────────────────
  const loadEntries = () => {
    if (!activePerson || personAccountIdsProp === undefined) return;
    apiPost<TaxEntry[]>("/tax/list", {
      person_id: activePerson.person_id,
      fy,
    }).then(setEntries).catch(() => setEntries([]));
  };
  useEffect(() => { loadEntries(); }, [activePerson?.person_id, fy, personAccountIdsProp]);

  // ── Tax computations ───────────────────────────────────────────────────────
  const taxDue = cgSummary ? computeTax(cgSummary) : null;

  const taxPaidByType = useMemo(() => {
    const paid: Record<string, number> = { TDS: 0, ADVANCE_TAX: 0, SELF_ASSESSMENT_TAX: 0 };
    for (const e of entries) {
      if (paid[e.entry_type] !== undefined) paid[e.entry_type] += e.amount_paise;
    }
    return paid;
  }, [entries]);

  const totalPaid = Object.values(taxPaidByType).reduce((s, v) => s + v, 0);
  const balanceDue = taxDue ? taxDue.total - totalPaid : null;

  // ── Available FYs ─────────────────────────────────────────────────────────
  const displayFYs = useMemo(() => {
    const set = new Set([...allCgFYs, currentFY]);
    return Array.from(set).sort().reverse();
  }, [allCgFYs]);

  // ── Account toggle helpers ────────────────────────────────────────────────
  const toggleAccount = (id: number) => {
    setSelectedAccountIds(prev =>
      prev.includes(id) ? prev.filter(x => x !== id) : [...prev, id]
    );
  };

  const allSelected = visibleAccounts.length > 0 &&
    visibleAccounts.every(a => selectedAccountIds.includes(a.account_id));

  const toggleAll = () => {
    if (allSelected) setSelectedAccountIds([]);
    else setSelectedAccountIds(visibleAccounts.map(a => a.account_id));
  };

  // ── Add entry dialog ───────────────────────────────────────────────────────
  const resetForm = () => {
    setFEntryType("TDS"); setFAmount("");
    setFDate(today); setFFY(dateToFY(today)); setFNotes(""); setError("");
  };

  const handleAdd = async () => {
    if (!activePerson)                         { setError("No active person"); return; }
    if (!fAmount || parseFloat(fAmount) <= 0)  { setError("Enter a valid amount"); return; }
    setSaving(true); setError("");
    try {
      await apiPost("/tax", {
        person_id:    activePerson.person_id,
        entry_type:   fEntryType,
        amount_paise: rupeesToPaise(fAmount),
        entry_date:   fDate,
        fy:           fFY,
        txn_id:       null,
        notes:        fNotes || null,
      });
      resetForm(); setShowAdd(false); loadEntries();
    } catch (e: any) {
      setError(e.toString());
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!delEntry) return;
    try {
      await apiDel(`/tax/${delEntry.entry_id}`);
      setDelEntry(null); loadEntries();
    } catch (e: any) {
      setError(e.toString());
    }
  };

  // ── Render ─────────────────────────────────────────────────────────────────
  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Tax</h1>
        <Button size="sm" onClick={() => { resetForm(); setShowAdd(true); }}>
          <Plus className="h-4 w-4 mr-1" /> Add Entry
        </Button>
      </div>

      {/* FY selector */}
      <div className="flex gap-1.5 flex-wrap">
        {displayFYs.map(f => (
          <button
            key={f}
            onClick={() => setFY(f)}
            className={cn(
              "px-3 py-1 rounded-full text-xs font-medium border transition-colors",
              fy === f
                ? "bg-primary text-primary-foreground border-primary"
                : "bg-background text-muted-foreground border-border hover:border-foreground/40",
            )}
          >
            FY {f}
          </button>
        ))}
      </div>

      {/* Portfolio + account filter */}
      <div className="flex flex-wrap items-center gap-3">
        {portfolios.length > 1 && (
          <Select value={selectedPortfolioId} onValueChange={v => v && setSelectedPortfolioId(v)}>
            <SelectTrigger className="w-44 h-8 text-sm">
              <SelectValue placeholder="All portfolios" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All portfolios</SelectItem>
              {portfolios.map(p => (
                <SelectItem key={p.portfolio_id} value={String(p.portfolio_id)}>{p.name}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}

        {visibleAccounts.length > 0 && (
          <div className="flex flex-wrap items-center gap-2">
            {visibleAccounts.length > 1 && (
              <button
                onClick={toggleAll}
                className="text-xs text-muted-foreground underline underline-offset-2 hover:text-foreground"
              >
                {allSelected ? "Deselect all" : "Select all"}
              </button>
            )}
            {visibleAccounts.map(a => (
              <label key={a.account_id} className="flex items-center gap-1.5 cursor-pointer">
                <input
                  type="checkbox"
                  checked={selectedAccountIds.includes(a.account_id)}
                  onChange={() => toggleAccount(a.account_id)}
                  className="h-3.5 w-3.5"
                />
                <span className="text-sm">{a.name}</span>
              </label>
            ))}
          </div>
        )}
      </div>

      {/* Tax liability card */}
      <Card>
        <CardContent className="py-3 px-4">
          <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide mb-3">
            Tax Liability — FY {fy}
          </p>
          {cgLoading ? (
            <p className="text-sm text-muted-foreground">Loading…</p>
          ) : cgSummary ? (
            <table className="w-full text-sm">
              <thead>
                <tr className="text-xs text-muted-foreground border-b">
                  <th className="text-left pb-1.5 font-normal">Category</th>
                  <th className="text-right pb-1.5 font-normal pr-6">Gain / Loss</th>
                  <th className="text-right pb-1.5 font-normal">Tax</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-border/40">
                <tr>
                  <td className="py-1.5">
                    <span className="font-medium">STCG – Equity</span>
                    <span className="text-xs text-muted-foreground ml-1.5">@ 20%</span>
                  </td>
                  <td className={cn("py-1.5 text-right pr-6", cgSummary.stcg_equity_paise < 0 && "text-red-500")}>
                    {formatINR(cgSummary.stcg_equity_paise)}
                  </td>
                  <td className="py-1.5 text-right font-medium">
                    {taxDue!.stcg_tax > 0 ? formatINR(taxDue!.stcg_tax) : "—"}
                  </td>
                </tr>
                <tr>
                  <td className="py-1.5">
                    <span className="font-medium">LTCG – Equity</span>
                    <span className="text-xs text-muted-foreground ml-1.5">@ 12.5% (₹1.25L exempt)</span>
                  </td>
                  <td className={cn("py-1.5 text-right pr-6", cgSummary.ltcg_equity_paise < 0 && "text-red-500")}>
                    {formatINR(cgSummary.ltcg_equity_paise)}
                  </td>
                  <td className="py-1.5 text-right font-medium">
                    {taxDue!.ltcg_equity_tax > 0 ? formatINR(taxDue!.ltcg_equity_tax) : "—"}
                  </td>
                </tr>
                <tr>
                  <td className="py-1.5">
                    <span className="font-medium">LTCG – Debt</span>
                    <span className="text-xs text-muted-foreground ml-1.5">@ 12.5%</span>
                  </td>
                  <td className={cn("py-1.5 text-right pr-6", cgSummary.ltcg_debt_paise < 0 && "text-red-500")}>
                    {formatINR(cgSummary.ltcg_debt_paise)}
                  </td>
                  <td className="py-1.5 text-right font-medium">
                    {taxDue!.ltcg_debt_tax > 0 ? formatINR(taxDue!.ltcg_debt_tax) : "—"}
                  </td>
                </tr>
                {(cgSummary.stcg_debt_paise !== 0 || cgSummary.speculative_paise !== 0 || cgSummary.non_speculative_paise !== 0) && (
                  <tr>
                    <td className="py-1.5">
                      <span className="font-medium">Slab rate items</span>
                      <span className="text-xs text-muted-foreground ml-1.5">STCG Debt · Intraday · F&O</span>
                    </td>
                    <td className="py-1.5 text-right pr-6 text-muted-foreground">
                      {formatINR(cgSummary.stcg_debt_paise + cgSummary.speculative_paise + cgSummary.non_speculative_paise)}
                    </td>
                    <td className="py-1.5 text-right text-xs text-muted-foreground">at slab</td>
                  </tr>
                )}
                <tr className="border-t border-border">
                  <td className="pt-2 pb-1 font-semibold">Total computed tax</td>
                  <td />
                  <td className="pt-2 pb-1 text-right font-semibold text-lg">
                    {formatINR(taxDue!.total)}
                  </td>
                </tr>
              </tbody>
            </table>
          ) : (
            <p className="text-sm text-muted-foreground">
              {selectedAccountIds.length === 0
                ? "Select at least one account."
                : "No capital gains data for this FY."}
            </p>
          )}
        </CardContent>
      </Card>

      {/* Tax paid + balance due */}
      <div className="grid grid-cols-2 gap-3">
        <Card>
          <CardContent className="py-3 px-4">
            <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide mb-3">
              Tax Paid / Withheld — FY {fy}
            </p>
            <table className="w-full text-sm">
              <tbody className="divide-y divide-border/40">
                {(["TDS", "ADVANCE_TAX", "SELF_ASSESSMENT_TAX"] as const).map(type => (
                  <tr key={type}>
                    <td className="py-1.5 text-muted-foreground">{ENTRY_TYPE_LABELS[type]}</td>
                    <td className="py-1.5 text-right font-medium">
                      {taxPaidByType[type] > 0 ? formatINR(taxPaidByType[type]) : "—"}
                    </td>
                  </tr>
                ))}
                <tr className="border-t border-border">
                  <td className="pt-2 pb-1 font-semibold">Total</td>
                  <td className="pt-2 pb-1 text-right font-semibold">{formatINR(totalPaid)}</td>
                </tr>
              </tbody>
            </table>
          </CardContent>
        </Card>

        <Card>
          <CardContent className="py-3 px-4 flex flex-col justify-center h-full min-h-[120px]">
            <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide mb-2">
              Balance Due
            </p>
            {balanceDue !== null ? (
              <>
                <p className={cn(
                  "text-3xl font-bold",
                  balanceDue > 0 ? "text-red-500" : "text-green-600 dark:text-green-400",
                )}>
                  {formatINR(Math.abs(balanceDue))}
                </p>
                <p className="text-xs text-muted-foreground mt-1">
                  {balanceDue > 0 ? "still to pay" : balanceDue < 0 ? "overpaid / refund" : "fully paid"}
                </p>
              </>
            ) : (
              <p className="text-sm text-muted-foreground">Select accounts above</p>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Ledger table */}
      <div className="flex items-center justify-between pt-1">
        <h2 className="text-sm font-semibold text-muted-foreground uppercase tracking-wide">Payment Entries</h2>
      </div>

      {entries.length === 0 ? (
        <p className="text-sm text-muted-foreground py-4 text-center">
          No tax entries for FY {fy}.
        </p>
      ) : (
        <div className="border rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-muted/50 text-xs text-muted-foreground">
              <tr>
                <th className="text-left px-3 py-2">Date</th>
                <th className="text-left px-3 py-2">Type</th>
                <th className="text-left px-3 py-2">FY</th>
                <th className="text-right px-3 py-2">Amount</th>
                <th className="text-left px-3 py-2">Notes</th>
                <th className="px-3 py-2" />
              </tr>
            </thead>
            <tbody>
              {entries.map((e, i) => (
                <tr key={e.entry_id} className={i % 2 === 0 ? "bg-background" : "bg-muted/20"}>
                  <td className="px-3 py-2 whitespace-nowrap">{formatDate(e.entry_date)}</td>
                  <td className="px-3 py-2">
                    <span className={`text-xs px-2 py-0.5 rounded-full font-medium ${ENTRY_TYPE_COLORS[e.entry_type] ?? ""}`}>
                      {ENTRY_TYPE_LABELS[e.entry_type] ?? e.entry_type}
                    </span>
                  </td>
                  <td className="px-3 py-2 text-muted-foreground">{e.fy}</td>
                  <td className="px-3 py-2 text-right font-medium">{formatINR(e.amount_paise)}</td>
                  <td className="px-3 py-2 text-muted-foreground max-w-xs truncate">{e.notes ?? "—"}</td>
                  <td className="px-3 py-2">
                    <Button
                      variant="ghost" size="icon"
                      className="h-7 w-7 text-muted-foreground hover:text-destructive"
                      onClick={() => setDelEntry(e)}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Add dialog */}
      <Dialog open={showAdd} onOpenChange={(o) => { if (!o) resetForm(); setShowAdd(o); }}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>Add Tax Entry</DialogTitle>
          </DialogHeader>
          <div className="space-y-3 py-1">
            <div className="space-y-1.5">
              <Label>Entry type</Label>
              <Select value={fEntryType} onValueChange={v => v && setFEntryType(v)}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="TDS">TDS</SelectItem>
                  <SelectItem value="ADVANCE_TAX">Advance Tax</SelectItem>
                  <SelectItem value="SELF_ASSESSMENT_TAX">Self Assessment Tax</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label>Amount (₹)</Label>
                <Input
                  type="number" min="0" step="0.01" placeholder="0.00"
                  value={fAmount} onChange={e => setFAmount(e.target.value)}
                />
              </div>
              <div className="space-y-1.5">
                <Label>Date</Label>
                <Input
                  type="date" value={fDate}
                  onChange={e => { setFDate(e.target.value); if (e.target.value) setFFY(dateToFY(e.target.value)); }}
                />
              </div>
            </div>
            <div className="space-y-1.5">
              <Label>Financial year</Label>
              <Input
                placeholder="e.g. 2025-26" value={fFY}
                onChange={e => setFFY(e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs">Notes <span className="text-muted-foreground">(optional)</span></Label>
              <Input
                placeholder="e.g. Advance tax Q1" value={fNotes}
                onChange={e => setFNotes(e.target.value)}
              />
            </div>
            {error && <p className="text-destructive text-sm">{error}</p>}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => { resetForm(); setShowAdd(false); }}>Cancel</Button>
            <Button onClick={handleAdd} disabled={saving}>{saving ? "Saving…" : "Add Entry"}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete confirm */}
      <AlertDialog open={!!delEntry} onOpenChange={o => { if (!o) setDelEntry(null); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete tax entry?</AlertDialogTitle>
            <AlertDialogDescription>
              {delEntry && (
                <>{ENTRY_TYPE_LABELS[delEntry.entry_type]} of {formatINR(delEntry.amount_paise)} on {formatDate(delEntry.entry_date)} will be permanently removed.</>
              )}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleDelete}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
