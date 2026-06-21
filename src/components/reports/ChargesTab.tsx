import { useEffect, useState, useMemo } from "react";
import { apiGet, apiPost, apiPut, apiDel } from "@/lib/api";
import { Plus, Pencil, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter,
} from "@/components/ui/dialog";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { formatINR, formatDate } from "@/lib/format";
import { cn } from "@/lib/utils";

interface Account {
  account_id: number;
  name: string;
}

interface Charge {
  charge_id: number;
  account_id: number;
  account_name: string;
  start_date: string;
  end_date: string;
  charge_type: string;
  amount_paise: number;
  source: string;
  import_batch_id: number | null;
  notes: string | null;
}

const CHARGE_TYPES = [
  "BROKERAGE", "STT", "GST", "EXCHANGE", "SEBI", "STAMP_DUTY", "DP", "OTHER",
] as const;

const CHARGE_TYPE_LABELS: Record<string, string> = {
  BROKERAGE: "Brokerage",
  STT:       "STT",
  GST:       "GST",
  EXCHANGE:  "Exchange Charges",
  SEBI:      "SEBI Charges",
  STAMP_DUTY:"Stamp Duty",
  DP:        "DP Charges",
  OTHER:     "Other",
};

// ── Charge dialog ─────────────────────────────────────────────────────────────

interface DialogState {
  open: boolean;
  charge: Charge | null;  // null = new
}

function ChargeDialog({
  state, accounts, onClose, onSaved,
}: {
  state: DialogState;
  accounts: Account[];
  onClose: () => void;
  onSaved: (c: Charge) => void;
}) {
  const editing = state.charge;

  const [accountId, setAccountId]   = useState<string>("");
  const [startDate, setStartDate]   = useState<string>("");
  const [endDate, setEndDate]       = useState<string>("");
  const [chargeType, setChargeType] = useState<string>("");
  const [amountStr, setAmountStr]   = useState<string>("");
  const [notes, setNotes]           = useState<string>("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError]           = useState<string | null>(null);

  useEffect(() => {
    if (!state.open) return;
    if (editing) {
      setAccountId(String(editing.account_id));
      setStartDate(editing.start_date);
      setEndDate(editing.end_date);
      setChargeType(editing.charge_type);
      setAmountStr((editing.amount_paise / 100).toFixed(2));
      setNotes(editing.notes ?? "");
    } else {
      setAccountId(accounts[0] ? String(accounts[0].account_id) : "");
      setStartDate("");
      setEndDate("");
      setChargeType("BROKERAGE");
      setAmountStr("");
      setNotes("");
    }
    setError(null);
  }, [state.open, editing]);

  const handleSubmit = async () => {
    setError(null);
    const amountPaise = Math.round(parseFloat(amountStr) * 100);
    if (!accountId)             { setError("Select an account."); return; }
    if (!startDate)             { setError("Enter a start date."); return; }
    if (!endDate)               { setError("Enter an end date."); return; }
    if (startDate > endDate)    { setError("Start date must be ≤ end date."); return; }
    if (!chargeType)            { setError("Select a charge type."); return; }
    if (isNaN(amountPaise) || amountPaise <= 0) { setError("Enter a valid amount."); return; }

    const input = {
      account_id:   parseInt(accountId),
      start_date:   startDate,
      end_date:     endDate,
      charge_type:  chargeType,
      amount_paise: amountPaise,
      notes:        notes.trim() || null,
    };

    setSubmitting(true);
    try {
      let saved: Charge;
      if (editing) {
        saved = await apiPut<Charge>(`/charges/${editing.charge_id}`, input);
      } else {
        saved = await apiPost<Charge>("/charges", input);
      }
      onSaved(saved);
    } catch (e: unknown) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={state.open} onOpenChange={(v) => { if (!v) onClose(); }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{editing ? "Edit Charge" : "Add Charge"}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* Account */}
          <div className="space-y-1.5">
            <Label>Account</Label>
            <Select value={accountId} onValueChange={(v) => setAccountId(v ?? "")}>
              <SelectTrigger>
                <SelectValue placeholder="Select account…" />
              </SelectTrigger>
              <SelectContent>
                {accounts.map((a) => (
                  <SelectItem key={a.account_id} value={String(a.account_id)}>
                    {a.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Date range */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>Start Date</Label>
              <Input type="date" value={startDate} onChange={e => setStartDate(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label>End Date</Label>
              <Input type="date" value={endDate} onChange={e => setEndDate(e.target.value)} />
            </div>
          </div>

          {/* Charge type */}
          <div className="space-y-1.5">
            <Label>Charge Type</Label>
            <Select value={chargeType} onValueChange={(v) => setChargeType(v ?? "")}>
              <SelectTrigger>
                <SelectValue placeholder="Select type…" />
              </SelectTrigger>
              <SelectContent>
                {CHARGE_TYPES.map((ct) => (
                  <SelectItem key={ct} value={ct}>{CHARGE_TYPE_LABELS[ct]}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Amount */}
          <div className="space-y-1.5">
            <Label>Amount (₹)</Label>
            <Input
              type="number"
              min="0"
              step="0.01"
              value={amountStr}
              onChange={e => setAmountStr(e.target.value)}
              placeholder="0.00"
            />
          </div>

          {/* Notes */}
          <div className="space-y-1.5">
            <Label>Notes (optional)</Label>
            <Input
              value={notes}
              onChange={e => setNotes(e.target.value)}
              placeholder="e.g. Jan 2024 ICICI contract notes"
            />
          </div>

          {error && <p className="text-sm text-destructive">{error}</p>}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={submitting}>Cancel</Button>
          <Button onClick={handleSubmit} disabled={submitting}>
            {submitting ? "Saving…" : editing ? "Save" : "Add"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Main tab ──────────────────────────────────────────────────────────────────

export function ChargesTab() {
  const [charges, setCharges]   = useState<Charge[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [loading, setLoading]   = useState(true);
  const [dialog, setDialog]     = useState<DialogState>({ open: false, charge: null });
  const [deleting, setDeleting] = useState<number | null>(null);

  const load = async () => {
    setLoading(true);
    try {
      const [cs, accts] = await Promise.all([
        apiPost<Charge[]>("/charges/list", { account_ids: null }),
        apiGet<Account[]>("/accounts"),
      ]);
      setCharges(cs);
      setAccounts(accts);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { load(); }, []);

  const total = useMemo(() => charges.reduce((s, c) => s + c.amount_paise, 0), [charges]);

  const handleDelete = async (id: number) => {
    setDeleting(id);
    try {
      await apiDel(`/charges/${id}`);
      setCharges(prev => prev.filter(c => c.charge_id !== id));
    } finally {
      setDeleting(null);
    }
  };

  const handleSaved = (saved: Charge) => {
    setCharges(prev => {
      const idx = prev.findIndex(c => c.charge_id === saved.charge_id);
      if (idx >= 0) {
        const next = [...prev];
        next[idx] = saved;
        return next;
      }
      return [saved, ...prev];
    });
    setDialog({ open: false, charge: null });
  };

  return (
    <div className="flex flex-col gap-4 h-full">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Charges</h1>
          <p className="text-sm text-muted-foreground">Trading costs by period — brokerage, STT, GST, and more</p>
        </div>
        <Button onClick={() => setDialog({ open: true, charge: null })} size="sm">
          <Plus className="size-4 mr-1.5" /> Add Charge
        </Button>
      </div>

      {/* Summary strip */}
      {charges.length > 0 && (
        <div className="flex items-center gap-6 rounded-lg border px-4 py-3 bg-muted/30">
          <div>
            <p className="text-xs text-muted-foreground">Total charges</p>
            <p className="text-lg font-semibold tabular-nums">{formatINR(total)}</p>
          </div>
          <div className="h-8 w-px bg-border" />
          {Object.entries(
            charges.reduce<Record<string, number>>((acc, c) => {
              acc[c.charge_type] = (acc[c.charge_type] ?? 0) + c.amount_paise;
              return acc;
            }, {})
          )
            .sort((a, b) => b[1] - a[1])
            .slice(0, 5)
            .map(([type, paise]) => (
              <div key={type}>
                <p className="text-xs text-muted-foreground">{CHARGE_TYPE_LABELS[type] ?? type}</p>
                <p className="text-sm font-medium tabular-nums">{formatINR(paise)}</p>
              </div>
            ))}
        </div>
      )}

      {/* Table */}
      <div className="flex-1 border rounded-lg overflow-hidden">
        <div className="overflow-auto h-full">
          <table className="w-full text-sm">
            <thead className="bg-muted sticky top-0 z-10">
              <tr>
                {["Account", "Period", "Type", "Amount", "Source", "Notes", ""].map((h, i) => (
                  <th
                    key={i}
                    className={cn(
                      "px-3 py-2.5 text-xs font-medium text-muted-foreground whitespace-nowrap text-left",
                      (h === "Amount") && "text-right",
                    )}
                  >
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="divide-y">
              {loading ? (
                <tr>
                  <td colSpan={7} className="px-3 py-8 text-center text-muted-foreground text-sm">
                    Loading…
                  </td>
                </tr>
              ) : charges.length === 0 ? (
                <tr>
                  <td colSpan={7} className="px-3 py-12 text-center">
                    <p className="text-muted-foreground text-sm">No charges recorded yet.</p>
                    <p className="text-xs text-muted-foreground mt-1">
                      Add charges manually from your broker's contract notes or statements.
                    </p>
                  </td>
                </tr>
              ) : (
                charges.map((c) => (
                  <tr key={c.charge_id} className="hover:bg-muted/30 transition-colors">
                    <td className="px-3 py-2.5 text-sm text-muted-foreground whitespace-nowrap">
                      {c.account_name}
                    </td>
                    <td className="px-3 py-2.5 text-sm whitespace-nowrap">
                      {c.start_date === c.end_date
                        ? formatDate(c.start_date)
                        : <>{formatDate(c.start_date)}<span className="text-muted-foreground"> – </span>{formatDate(c.end_date)}</>}
                    </td>
                    <td className="px-3 py-2.5">
                      <span className="text-xs px-1.5 py-0.5 rounded bg-muted font-medium">
                        {CHARGE_TYPE_LABELS[c.charge_type] ?? c.charge_type}
                      </span>
                    </td>
                    <td className="px-3 py-2.5 text-right tabular-nums font-medium">
                      {formatINR(c.amount_paise)}
                    </td>
                    <td className="px-3 py-2.5">
                      <span className={cn(
                        "text-xs px-1.5 py-0.5 rounded font-medium",
                        c.source === "IMPORT"
                          ? "bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400"
                          : "bg-muted text-muted-foreground"
                      )}>
                        {c.source === "IMPORT" ? "Imported" : "Manual"}
                      </span>
                    </td>
                    <td className="px-3 py-2.5 text-sm text-muted-foreground max-w-[200px] truncate">
                      {c.notes ?? ""}
                    </td>
                    <td className="px-3 py-2.5">
                      <div className="flex items-center gap-1 justify-end">
                        <button
                          onClick={() => setDialog({ open: true, charge: c })}
                          className="p-1 rounded hover:bg-muted transition-colors text-muted-foreground hover:text-foreground"
                        >
                          <Pencil className="size-3.5" />
                        </button>
                        <button
                          onClick={() => handleDelete(c.charge_id)}
                          disabled={deleting === c.charge_id}
                          className="p-1 rounded hover:bg-destructive/10 transition-colors text-muted-foreground hover:text-destructive disabled:opacity-40"
                        >
                          <Trash2 className="size-3.5" />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </div>

      <ChargeDialog
        state={dialog}
        accounts={accounts}
        onClose={() => setDialog({ open: false, charge: null })}
        onSaved={handleSaved}
      />
    </div>
  );
}
