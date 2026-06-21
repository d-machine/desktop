import { useEffect, useState } from "react";
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

interface Person    { person_id: number; name: string; }
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

const today = new Date().toISOString().split("T")[0];

export function TaxPage() {
  const [persons,  setPersons]  = useState<Person[]>([]);
  const [entries,  setEntries]  = useState<TaxEntry[]>([]);
  const [filterPerson, setFilterPerson] = useState<string>("all");
  const [filterFY,     setFilterFY]     = useState<string>("all");

  const [showAdd,  setShowAdd]  = useState(false);
  const [delEntry, setDelEntry] = useState<TaxEntry | null>(null);
  const [saving,   setSaving]   = useState(false);
  const [error,    setError]    = useState("");

  // Add-form state
  const [fPersonId,   setFPersonId]   = useState("");
  const [fEntryType,  setFEntryType]  = useState("TDS");
  const [fAmount,     setFAmount]     = useState("");
  const [fDate,       setFDate]       = useState(today);
  const [fFY,         setFFY]         = useState(dateToFY(today));
  const [fNotes,      setFNotes]      = useState("");

  const load = () => {
    setError("");
    apiPost<TaxEntry[]>("/tax/list", {
      person_id: filterPerson !== "all" ? parseInt(filterPerson) : null,
      fy:        filterFY     !== "all" ? filterFY               : null,
    }).then(setEntries).catch((e: unknown) => {
      setEntries([]);
      setError(e instanceof Error ? e.message : String(e));
    });
  };

  useEffect(() => {
    apiGet<Person[]>("/persons").then(setPersons).catch((e: unknown) => {
      setError(e instanceof Error ? e.message : String(e));
    });
  }, []);

  useEffect(() => { load(); }, [filterPerson, filterFY]);

  // Derive unique FYs from loaded entries for the FY filter dropdown
  const allFYs = Array.from(new Set(entries.map(e => e.fy))).sort().reverse();

  const totalPaise = entries.reduce((s, e) => s + e.amount_paise, 0);

  const resetForm = () => {
    setFPersonId("");
    setFEntryType("TDS");
    setFAmount("");
    setFDate(today);
    setFFY(dateToFY(today));
    setFNotes("");
    setError("");
  };

  const handleAdd = async () => {
    if (!fPersonId)          { setError("Select a person"); return; }
    if (!fAmount || parseFloat(fAmount) <= 0) { setError("Enter a valid amount"); return; }

    setSaving(true);
    setError("");
    try {
      await apiPost("/tax", {
        person_id:    parseInt(fPersonId),
        entry_type:   fEntryType,
        amount_paise: rupeesToPaise(fAmount),
        entry_date:   fDate,
        fy:           fFY,
        txn_id:       null,
        notes:        fNotes || null,
      });
      resetForm();
      setShowAdd(false);
      load();
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
      setDelEntry(null);
      load();
    } catch (e: any) {
      setError(e.toString());
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Tax Ledger</h1>
        <Button size="sm" onClick={() => { resetForm(); setShowAdd(true); }}>
          <Plus className="h-4 w-4 mr-1" /> Add Entry
        </Button>
      </div>

      {/* Summary card */}
      <Card>
        <CardContent className="py-3 px-4 flex gap-8">
          <div>
            <p className="text-xs text-muted-foreground">Total tax paid / withheld</p>
            <p className="text-lg font-semibold">{formatINR(totalPaise)}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">Entries</p>
            <p className="text-lg font-semibold">{entries.length}</p>
          </div>
        </CardContent>
      </Card>

      {/* Filters */}
      <div className="flex gap-3 flex-wrap">
        <Select value={filterPerson} onValueChange={(v) => v && setFilterPerson(v)}>
          <SelectTrigger className="w-44 h-8 text-sm">
            <SelectValue placeholder="All persons" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All persons</SelectItem>
            {persons.map(p => (
              <SelectItem key={p.person_id} value={String(p.person_id)}>{p.name}</SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select value={filterFY} onValueChange={(v) => v && setFilterFY(v)}>
          <SelectTrigger className="w-36 h-8 text-sm">
            <SelectValue placeholder="All years" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All years</SelectItem>
            {allFYs.map(fy => (
              <SelectItem key={fy} value={fy}>FY {fy}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* Table */}
      {error && (
        <div className="text-sm text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">
          {error}
        </div>
      )}

      {entries.length === 0 ? (
        <p className="text-sm text-muted-foreground py-8 text-center">No tax entries found.</p>
      ) : (
        <div className="border rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-muted/50 text-xs text-muted-foreground">
              <tr>
                <th className="text-left px-3 py-2">Date</th>
                <th className="text-left px-3 py-2">Person</th>
                <th className="text-left px-3 py-2">Type</th>
                <th className="text-left px-3 py-2">FY</th>
                <th className="text-right px-3 py-2">Amount</th>
                <th className="text-left px-3 py-2">Notes</th>
                <th className="text-left px-3 py-2">Linked Txn</th>
                <th className="px-3 py-2" />
              </tr>
            </thead>
            <tbody>
              {entries.map((e, i) => (
                <tr key={e.entry_id} className={i % 2 === 0 ? "bg-background" : "bg-muted/20"}>
                  <td className="px-3 py-2 whitespace-nowrap">{formatDate(e.entry_date)}</td>
                  <td className="px-3 py-2">{e.person_name}</td>
                  <td className="px-3 py-2">
                    <span className={`text-xs px-2 py-0.5 rounded-full font-medium ${ENTRY_TYPE_COLORS[e.entry_type] ?? ""}`}>
                      {ENTRY_TYPE_LABELS[e.entry_type] ?? e.entry_type}
                    </span>
                  </td>
                  <td className="px-3 py-2 text-muted-foreground">{e.fy}</td>
                  <td className="px-3 py-2 text-right font-medium">{formatINR(e.amount_paise)}</td>
                  <td className="px-3 py-2 text-muted-foreground max-w-xs truncate">{e.notes ?? "—"}</td>
                  <td className="px-3 py-2 text-muted-foreground">
                    {e.txn_id ? <span className="text-xs font-mono">#{e.txn_id}</span> : "—"}
                  </td>
                  <td className="px-3 py-2">
                    <Button
                      variant="ghost"
                      size="icon"
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
              <Label>Person</Label>
              <Select value={fPersonId} onValueChange={(v) => v && setFPersonId(v)}>
                <SelectTrigger><SelectValue placeholder="Select person" /></SelectTrigger>
                <SelectContent>
                  {persons.map(p => (
                    <SelectItem key={p.person_id} value={String(p.person_id)}>{p.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-1.5">
              <Label>Entry type</Label>
              <Select value={fEntryType} onValueChange={(v) => v && setFEntryType(v)}>
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
                  type="number"
                  min="0"
                  step="0.01"
                  placeholder="0.00"
                  value={fAmount}
                  onChange={(e) => setFAmount(e.target.value)}
                />
              </div>
              <div className="space-y-1.5">
                <Label>Date</Label>
                <Input
                  type="date"
                  value={fDate}
                  onChange={(e) => {
                    setFDate(e.target.value);
                    if (e.target.value) setFFY(dateToFY(e.target.value));
                  }}
                />
              </div>
            </div>

            <div className="space-y-1.5">
              <Label>Financial year</Label>
              <Input
                placeholder="e.g. 2025-26"
                value={fFY}
                onChange={(e) => setFFY(e.target.value)}
              />
            </div>

            <div className="space-y-1.5">
              <Label className="text-xs">Notes <span className="text-muted-foreground">(optional)</span></Label>
              <Input
                placeholder="e.g. Advance tax Q1"
                value={fNotes}
                onChange={(e) => setFNotes(e.target.value)}
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
      <AlertDialog open={!!delEntry} onOpenChange={(o) => { if (!o) setDelEntry(null); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete tax entry?</AlertDialogTitle>
            <AlertDialogDescription>
              {delEntry && (
                <>
                  {ENTRY_TYPE_LABELS[delEntry.entry_type]} of {formatINR(delEntry.amount_paise)} on {formatDate(delEntry.entry_date)} will be permanently removed.
                </>
              )}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={handleDelete} className="bg-destructive text-destructive-foreground hover:bg-destructive/90">
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
