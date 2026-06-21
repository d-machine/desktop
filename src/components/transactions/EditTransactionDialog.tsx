import { useState, useEffect } from "react";
import { apiPatch } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter,
} from "@/components/ui/dialog";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { AlertTriangle } from "lucide-react";
import { TXN_TYPES, TRADE_SEGMENTS } from "@/lib/txn-types";
import { rupeesToPaise, paiseToRupees } from "@/lib/format";

interface Transaction {
  txn_id: number;
  instrument_name: string;
  account_name: string;
  txn_type: string;
  trade_segment: string;
  trade_date: string;
  txn_time?: string;
  quantity: number;
  effective_price_paise: number;
  actual_price_paise?: number;
  brokerage_per_unit_paise?: number;
  stt_paise: number;
  other_charges_paise: number;
  notes?: string;
  flag?: string;
  flag_reason?: string;
  flag_dismissed: boolean;
}

interface EditTransactionDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  transaction: Transaction | null;
  onSaved: () => void;
}

export function EditTransactionDialog({ open, onOpenChange, transaction, onSaved }: EditTransactionDialogProps) {
  const [txnType, setTxnType]     = useState("BUY");
  const [segment, setSegment]     = useState("DELIVERY");
  const [tradeDate, setTradeDate] = useState("");
  const [txnTime, setTxnTime]     = useState("");
  const [quantity, setQuantity]   = useState("");
  const [price, setPrice]         = useState("");
  const [brokerage, setBrokerage] = useState("0");
  const [stt, setStt]             = useState("0");
  const [otherCharges, setOtherCharges] = useState("0");
  const [notes, setNotes]         = useState("");
  const [saving, setSaving]       = useState(false);
  const [error, setError]         = useState("");

  // Populate fields from transaction whenever it changes
  useEffect(() => {
    if (!transaction) return;
    setTxnType(transaction.txn_type);
    setSegment(transaction.trade_segment);
    setTradeDate(transaction.trade_date);
    setTxnTime(transaction.txn_time ?? "");
    setQuantity(transaction.quantity.toString());
    setPrice(paiseToRupees(transaction.effective_price_paise).toString());
    setBrokerage("0");
    setStt(paiseToRupees(transaction.stt_paise).toString());
    setOtherCharges(paiseToRupees(transaction.other_charges_paise).toString());
    setNotes(transaction.notes ?? "");
    setError("");
  }, [transaction]);

  const selectedType = TXN_TYPES.find((t) => t.value === txnType);

  const totalPaise = (() => {
    const gross = parseFloat(quantity || "0") * rupeesToPaise(price);
    const charges = rupeesToPaise(brokerage) + rupeesToPaise(stt) + rupeesToPaise(otherCharges);
    if (["BUY", "SIP", "OPENING_BALANCE", "TRANSFER_IN"].includes(txnType)) return -(gross + charges);
    if (["SELL", "REDEMPTION", "TRANSFER_OUT"].includes(txnType)) return gross - charges;
    return gross;
  })();

  const handleSave = async () => {
    if (!transaction) return;
    if (!quantity || parseFloat(quantity) <= 0) { setError("Enter a valid quantity"); return; }
    if (selectedType?.showPrice && (!price || rupeesToPaise(price) <= 0)) { setError("Enter a valid price"); return; }

    setSaving(true);
    setError("");
    try {
      await apiPatch("/transactions", {
        txn_id: transaction.txn_id,
        txn_type: txnType,
        trade_segment: segment,
        trade_date: tradeDate,
        txn_time: txnTime || null,
        quantity: parseFloat(quantity),
        effective_price_paise: rupeesToPaise(price),
        actual_price_paise: null,
        brokerage_per_unit_paise: null,
        stt_paise: rupeesToPaise(stt),
        other_charges_paise: rupeesToPaise(otherCharges),
        notes: notes || null,
      });
      onOpenChange(false);
      onSaved();
    } catch (e: any) {
      setError(e.toString());
    } finally {
      setSaving(false);
    }
  };

  if (!transaction) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Edit Transaction</DialogTitle>
          <p className="text-sm text-muted-foreground">
            {transaction.instrument_name} · {transaction.account_name}
          </p>
        </DialogHeader>

        {/* Flag warning */}
        {transaction.flag && !transaction.flag_dismissed && (
          <div className="flex gap-2 items-start rounded-md bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800 px-3 py-2.5 text-sm">
            <AlertTriangle className="size-4 text-amber-500 mt-0.5 shrink-0" />
            <div>
              <p className="font-medium text-amber-700 dark:text-amber-400">{transaction.flag}</p>
              {transaction.flag_reason && (
                <p className="text-xs text-amber-600 dark:text-amber-500 mt-0.5">{transaction.flag_reason}</p>
              )}
              <p className="text-xs text-muted-foreground mt-1">
                Saving this edit will re-evaluate the flag automatically.
              </p>
            </div>
          </div>
        )}

        <div className="space-y-4 py-1">
          {/* Type + Segment */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>Transaction type</Label>
              <Select value={txnType} onValueChange={(v) => v && setTxnType(v)}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  {TXN_TYPES.map((t) => (
                    <SelectItem key={t.value} value={t.value}>{t.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label>Segment</Label>
              <Select value={segment} onValueChange={(v) => v && setSegment(v)}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  {TRADE_SEGMENTS.map((s) => (
                    <SelectItem key={s.value} value={s.value}>{s.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          {/* Date + Time */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>Trade date</Label>
              <Input type="date" value={tradeDate} onChange={(e) => setTradeDate(e.target.value)} />
            </div>
            {segment === "INTRADAY" && (
              <div className="space-y-1.5">
                <Label>Time <span className="text-muted-foreground text-xs">(IST)</span></Label>
                <Input type="time" value={txnTime} onChange={(e) => setTxnTime(e.target.value)} step="1" />
              </div>
            )}
          </div>

          {/* Quantity + Price */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>Quantity</Label>
              <Input
                type="number" min="0" step="0.001" placeholder="0"
                value={quantity} onChange={(e) => setQuantity(e.target.value)}
              />
            </div>
            {selectedType?.showPrice && (
              <div className="space-y-1.5">
                <Label>Price per unit (₹)</Label>
                <Input
                  type="number" min="0" step="0.01" placeholder="0.00"
                  value={price} onChange={(e) => setPrice(e.target.value)}
                />
              </div>
            )}
          </div>

          {/* Charges */}
          {selectedType?.showCharges && (
            <div className="grid grid-cols-3 gap-3">
              <div className="space-y-1.5">
                <Label className="text-xs">Brokerage (₹)</Label>
                <Input type="number" min="0" step="0.01" value={brokerage} onChange={(e) => setBrokerage(e.target.value)} />
              </div>
              <div className="space-y-1.5">
                <Label className="text-xs">STT (₹)</Label>
                <Input type="number" min="0" step="0.01" value={stt} onChange={(e) => setStt(e.target.value)} />
              </div>
              <div className="space-y-1.5">
                <Label className="text-xs">Other (₹)</Label>
                <Input type="number" min="0" step="0.01" value={otherCharges} onChange={(e) => setOtherCharges(e.target.value)} />
              </div>
            </div>
          )}

          {/* Total */}
          {quantity && price && (
            <div className="flex items-center justify-between bg-muted/40 rounded-md px-3 py-2 text-sm">
              <span className="text-muted-foreground">Total value</span>
              <span className={`font-semibold ${totalPaise < 0 ? "text-red-600 dark:text-red-400" : "text-green-600 dark:text-green-400"}`}>
                {totalPaise < 0 ? "−" : "+"}₹{Math.abs(paiseToRupees(totalPaise)).toLocaleString("en-IN", { minimumFractionDigits: 2 })}
              </span>
            </div>
          )}

          {/* Notes */}
          <div className="space-y-1.5">
            <Label className="text-xs">Notes <span className="text-muted-foreground">(optional)</span></Label>
            <Input placeholder="Any notes" value={notes} onChange={(e) => setNotes(e.target.value)} />
          </div>

          {error && <p className="text-destructive text-sm">{error}</p>}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button onClick={handleSave} disabled={saving}>{saving ? "Saving…" : "Save Changes"}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
