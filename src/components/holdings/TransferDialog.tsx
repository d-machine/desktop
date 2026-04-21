import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";

interface Holding {
  instrument_id: number;
  instrument_name: string;
  isin?: string;
  asset_class: string;
  account_id: number;
  account_name: string;
  quantity: number;
  avg_cost_paise: number;
}

interface Account {
  account_id: number;
  portfolio_id: number;
  name: string;
  account_type: string;
  broker?: string;
}

interface TransferResult {
  transfer_out_txn_id: number;
  transfer_in_txn_id: number;
}

interface Props {
  holding: Holding | null;
  accounts: Account[];
  onClose: () => void;
  onDone: () => void;
}

export function TransferDialog({ holding, accounts, onClose, onDone }: Props) {
  const open = holding !== null;

  const [toAccountId, setToAccountId] = useState<string>("");
  const [quantity, setQuantity] = useState<string>("");
  const [pricePerUnit, setPricePerUnit] = useState<string>("");
  const [tradeDate, setTradeDate] = useState<string>(() => new Date().toISOString().slice(0, 10));
  const [notes, setNotes] = useState<string>("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reset form whenever holding changes
  useEffect(() => {
    if (!holding) return;
    setToAccountId("");
    setQuantity(holding.quantity.toString());
    setPricePerUnit((holding.avg_cost_paise / 100).toFixed(2));
    setTradeDate(new Date().toISOString().slice(0, 10));
    setNotes("");
    setError(null);
  }, [holding]);

  if (!holding) return null;

  const destinationAccounts = accounts.filter(a => a.account_id !== holding.account_id);

  const tradeSegmentFor = (assetClass: string) => {
    if (assetClass === "MF") return "MF";
    if (assetClass === "EQUITY") return "DELIVERY";
    return "OTHER";
  };

  const handleSubmit = async () => {
    setError(null);
    const qty = parseFloat(quantity);
    const price = parseFloat(pricePerUnit);

    if (!toAccountId) { setError("Select a destination account."); return; }
    if (isNaN(qty) || qty <= 0) { setError("Enter a valid quantity."); return; }
    if (qty > holding.quantity) { setError(`Cannot transfer more than available quantity (${holding.quantity}).`); return; }
    if (isNaN(price) || price < 0) { setError("Enter a valid transfer price."); return; }
    if (!tradeDate) { setError("Enter a trade date."); return; }

    setSubmitting(true);
    try {
      await invoke<TransferResult>("transfer_holding", {
        input: {
          from_account_id: holding.account_id,
          to_account_id: parseInt(toAccountId),
          instrument_id: holding.instrument_id,
          quantity: qty,
          price_paise: Math.round(price * 100),
          trade_date: tradeDate,
          trade_segment: tradeSegmentFor(holding.asset_class),
          notes: notes.trim() || null,
        },
      });
      onDone();
    } catch (e: any) {
      setError(e?.toString() ?? "Transfer failed.");
    } finally {
      setSubmitting(false);
    }
  };

  const toAccount = accounts.find(a => a.account_id.toString() === toAccountId);

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!v) onClose(); }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Transfer Holding</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* Instrument */}
          <div className="rounded-md bg-muted/50 px-3 py-2.5 text-sm">
            <div className="font-medium">{holding.instrument_name}</div>
            <div className="text-xs text-muted-foreground mt-0.5">
              From <span className="font-medium text-foreground">{holding.account_name}</span>
              {" · "}Available: {holding.quantity}
            </div>
          </div>

          {/* Destination account */}
          <div className="space-y-1.5">
            <Label>To Account</Label>
            <Select value={toAccountId} onValueChange={(v) => setToAccountId(v ?? "")}>
              <SelectTrigger>
                <SelectValue placeholder="Select destination account" />
              </SelectTrigger>
              <SelectContent>
                {destinationAccounts.length === 0 ? (
                  <div className="px-3 py-2 text-sm text-muted-foreground">No other accounts</div>
                ) : (
                  destinationAccounts.map(a => (
                    <SelectItem key={a.account_id} value={a.account_id.toString()}>
                      {a.name}{a.broker ? ` · ${a.broker}` : ""}
                    </SelectItem>
                  ))
                )}
              </SelectContent>
            </Select>
          </div>

          {/* Quantity */}
          <div className="space-y-1.5">
            <Label>Quantity</Label>
            <Input
              type="number"
              min="0"
              step="any"
              value={quantity}
              onChange={e => setQuantity(e.target.value)}
              placeholder={`Max: ${holding.quantity}`}
            />
          </div>

          {/* Transfer price */}
          <div className="space-y-1.5">
            <Label>Transfer Price per Unit (₹)</Label>
            <Input
              type="number"
              min="0"
              step="0.01"
              value={pricePerUnit}
              onChange={e => setPricePerUnit(e.target.value)}
              placeholder="Enter price"
            />
            <p className="text-xs text-muted-foreground">
              Pre-filled with avg cost. Use 0 for off-market / gift transfers.
            </p>
          </div>

          {/* Date */}
          <div className="space-y-1.5">
            <Label>Transfer Date</Label>
            <Input
              type="date"
              value={tradeDate}
              onChange={e => setTradeDate(e.target.value)}
            />
          </div>

          {/* Notes */}
          <div className="space-y-1.5">
            <Label>Notes (optional)</Label>
            <Input
              value={notes}
              onChange={e => setNotes(e.target.value)}
              placeholder="e.g. Off-market transfer, gift"
            />
          </div>

          {/* Summary */}
          {toAccountId && quantity && pricePerUnit && (
            <div className="rounded-md border px-3 py-2.5 text-sm space-y-1">
              <div className="flex justify-between">
                <span className="text-muted-foreground">From</span>
                <span className="font-medium">{holding.account_name}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">To</span>
                <span className="font-medium">{toAccount?.name ?? "—"}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Value</span>
                <span className="font-medium tabular-nums">
                  ₹{(parseFloat(quantity || "0") * parseFloat(pricePerUnit || "0")).toLocaleString("en-IN", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                </span>
              </div>
            </div>
          )}

          {error && (
            <p className="text-sm text-destructive">{error}</p>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={submitting}>Cancel</Button>
          <Button onClick={handleSubmit} disabled={submitting}>
            {submitting ? "Transferring…" : "Transfer"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
