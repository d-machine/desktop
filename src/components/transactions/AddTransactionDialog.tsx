import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter,
} from "@/components/ui/dialog";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { InstrumentSearch } from "./InstrumentSearch";
import { TXN_TYPES, TRADE_SEGMENTS } from "@/lib/txn-types";
import { rupeesToPaise, paiseToRupees } from "@/lib/format";

interface Account { account_id: number; portfolio_id: number; name: string; account_type: string; }
interface InstrumentSummary { instrument_id: number; isin?: string; name: string; asset_class: string; nse_symbol?: string; }

interface AddTransactionDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  accounts: Account[];
  onSaved: () => void;
  defaultAccountId?: number;
}

const today = new Date().toISOString().split("T")[0];

export function AddTransactionDialog({ open, onOpenChange, accounts, onSaved, defaultAccountId }: AddTransactionDialogProps) {
  const [accountId, setAccountId] = useState<string>(defaultAccountId?.toString() ?? "");
  const [instrument, setInstrument] = useState<InstrumentSummary | null>(null);
  const [txnType, setTxnType] = useState("BUY");
  const [segment, setSegment] = useState("DELIVERY");
  const [tradeDate, setTradeDate] = useState(today);
  const [txnTime, setTxnTime] = useState("");
  const [quantity, setQuantity] = useState("");
  const [price, setPrice] = useState("");        // rupees
  const [brokerage, setBrokerage] = useState("0");
  const [stt, setStt] = useState("0");
  const [otherCharges, setOtherCharges] = useState("0");
  const [notes, setNotes] = useState("");
  const [brokerRef, setBrokerRef] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const selectedType = TXN_TYPES.find((t) => t.value === txnType)!;

  // Auto-set segment when type changes
  useEffect(() => {
    setSegment(selectedType.defaultSegment);
  }, [txnType]);

  // Computed total
  const totalPaise = (() => {
    const gross = parseFloat(quantity || "0") * rupeesToPaise(price);
    const charges = rupeesToPaise(brokerage) + rupeesToPaise(stt) + rupeesToPaise(otherCharges);
    if (["BUY", "SIP"].includes(txnType)) return -(gross + charges);
    if (["SELL", "REDEMPTION"].includes(txnType)) return gross - charges;
    return gross;
  })();

  const reset = () => {
    setInstrument(null);
    setTxnType("BUY");
    setSegment("DELIVERY");
    setTradeDate(today);
    setTxnTime("");
    setQuantity("");
    setPrice("");
    setBrokerage("0");
    setStt("0");
    setOtherCharges("0");
    setNotes("");
    setBrokerRef("");
    setError("");
  };

  const handleSave = async () => {
    if (!accountId) { setError("Select an account"); return; }
    if (!instrument) { setError("Select an instrument"); return; }
    if (!quantity || parseFloat(quantity) <= 0) { setError("Enter a valid quantity"); return; }
    if (selectedType.showPrice && (!price || rupeesToPaise(price) <= 0)) { setError("Enter a valid price"); return; }

    setSaving(true);
    setError("");
    try {
      await invoke("create_transaction", {
        input: {
          account_id: parseInt(accountId),
          instrument_id: instrument.instrument_id,
          txn_type: txnType,
          trade_segment: segment,
          trade_date: tradeDate,
          txn_time: txnTime || null,
          quantity: parseFloat(quantity),
          price_paise: rupeesToPaise(price),
          brokerage_paise: rupeesToPaise(brokerage),
          stt_paise: rupeesToPaise(stt),
          other_charges_paise: rupeesToPaise(otherCharges),
          notes: notes || null,
          broker_ref: brokerRef || null,
        },
      });
      reset();
      onOpenChange(false);
      onSaved();
    } catch (e: any) {
      setError(e.toString());
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) reset(); onOpenChange(o); }}>
      <DialogContent className="max-w-lg max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Add Transaction</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-1">
          {/* Account */}
          <div className="space-y-1.5">
            <Label>Account</Label>
            <Select value={accountId} onValueChange={setAccountId}>
              <SelectTrigger><SelectValue placeholder="Select account" /></SelectTrigger>
              <SelectContent>
                {accounts.map((a) => (
                  <SelectItem key={a.account_id} value={a.account_id.toString()}>{a.name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Instrument */}
          <div className="space-y-1.5">
            <Label>Instrument</Label>
            <InstrumentSearch value={instrument ?? undefined} onChange={setInstrument} />
          </div>

          {/* Type + Segment */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>Transaction type</Label>
              <Select value={txnType} onValueChange={setTxnType}>
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
              <Select value={segment} onValueChange={setSegment}>
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
                <Label>Time <span className="text-muted-foreground text-xs">(IST, for intraday)</span></Label>
                <Input type="time" value={txnTime} onChange={(e) => setTxnTime(e.target.value)} step="1" />
              </div>
            )}
          </div>

          {/* Quantity + Price */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>Quantity</Label>
              <Input
                type="number"
                min="0"
                step="0.001"
                placeholder="0"
                value={quantity}
                onChange={(e) => setQuantity(e.target.value)}
              />
            </div>
            {selectedType.showPrice && (
              <div className="space-y-1.5">
                <Label>Price per unit (₹)</Label>
                <Input
                  type="number"
                  min="0"
                  step="0.01"
                  placeholder="0.00"
                  value={price}
                  onChange={(e) => setPrice(e.target.value)}
                />
              </div>
            )}
          </div>

          {/* Charges */}
          {selectedType.showCharges && (
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

          {/* Notes + Broker Ref */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label className="text-xs">Notes <span className="text-muted-foreground">(optional)</span></Label>
              <Input placeholder="Any notes" value={notes} onChange={(e) => setNotes(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs">Broker ref <span className="text-muted-foreground">(optional)</span></Label>
              <Input placeholder="Trade ID from broker" value={brokerRef} onChange={(e) => setBrokerRef(e.target.value)} />
            </div>
          </div>

          {error && <p className="text-destructive text-sm">{error}</p>}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => { reset(); onOpenChange(false); }}>Cancel</Button>
          <Button onClick={handleSave} disabled={saving}>{saving ? "Saving…" : "Add Transaction"}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
