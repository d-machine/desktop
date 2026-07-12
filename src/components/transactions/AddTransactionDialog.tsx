import { useState, useEffect, useMemo } from "react";
import { apiGet, apiPost } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter,
} from "@/components/ui/dialog";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { InstrumentSearch, InstrumentSummary } from "./InstrumentSearch";
import { TXN_TYPES, TRADE_SEGMENTS } from "@/lib/txn-types";
import { rupeesToPaise, paiseToRupees, dateToFY } from "@/lib/format";
import {
  PersonPortfolioAccountSelector,
  type Person, type Portfolio, type Account, type Selection,
} from "@/components/shared/PersonPortfolioAccountSelector";

interface AddTransactionDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSaved: () => void;
}

const today = new Date().toISOString().split("T")[0];

// Per-asset-class display config
interface ClassConfig {
  availableTypes: string[];
  showSegment: boolean;
  forcedSegment: string | null;
  hideCharges: boolean;   // override — always hide brokerage/STT regardless of txn type
  quantityLabel: string;
  priceLabel: string;
}

function getClassConfig(assetClass: string): ClassConfig {
  switch (assetClass) {
    case "MF":
      return {
        availableTypes: [
          "SIP", "REDEMPTION", "DIVIDEND", "SWITCH_IN", "SWITCH_OUT",
          "OPENING_BALANCE", "TRANSFER_IN", "TRANSFER_OUT", "MERGER_IN", "MERGER_OUT",
        ],
        showSegment:   false,
        forcedSegment: "DELIVERY",
        hideCharges:   true,
        quantityLabel: "Units",
        priceLabel:    "NAV (₹)",
      };
    case "FUTSTK":
    case "FUTIDX":
      return {
        availableTypes: ["BUY", "SELL", "OPENING_BALANCE"],
        showSegment:   false,
        forcedSegment: "FNO",
        hideCharges:   false,
        quantityLabel: "Lots",
        priceLabel:    "Price per lot (₹)",
      };
    case "OPTSTK":
    case "OPTIDX":
      return {
        availableTypes: ["BUY", "SELL", "OPENING_BALANCE"],
        showSegment:   false,
        forcedSegment: "FNO",
        hideCharges:   false,
        quantityLabel: "Lots",
        priceLabel:    "Premium (₹)",
      };
    case "MCX":
      return {
        availableTypes: ["BUY", "SELL", "OPENING_BALANCE"],
        showSegment:   false,
        forcedSegment: "COMMODITY",
        hideCharges:   false,
        quantityLabel: "Lots",
        priceLabel:    "Price per lot (₹)",
      };
    default: // EQUITY, FIXED_INCOME, etc.
      return {
        availableTypes: [
          "BUY", "SELL", "IPO", "FPO",
          "BONUS", "SPLIT_OUT", "SPLIT_IN",
          "OPENING_BALANCE", "TRANSFER_IN", "TRANSFER_OUT",
          "MERGER_IN", "MERGER_OUT", "DIVIDEND", "INTEREST",
        ],
        showSegment:   true,
        forcedSegment: null,
        hideCharges:   false,
        quantityLabel: "Quantity",
        priceLabel:    "Price per unit (₹)",
      };
  }
}

export function AddTransactionDialog({ open, onOpenChange, onSaved }: AddTransactionDialogProps) {
  // Person → Portfolio → Account selection
  const [persons,     setPersons]     = useState<Person[]>([]);
  const [portfolios,  setPortfolios]  = useState<Portfolio[]>([]);
  const [allAccounts, setAllAccounts] = useState<Account[]>([]);
  const [selection,   setSelection]   = useState<Partial<Selection>>({});

  const accountId = selection.account?.account_id.toString() ?? "";

  const [instrument, setInstrument] = useState<InstrumentSummary | null>(null);
  const [txnType, setTxnType] = useState("BUY");
  const [segment, setSegment] = useState("DELIVERY");
  const [tradeDate, setTradeDate] = useState(today);
  const [txnTime, setTxnTime] = useState("");
  const [quantity, setQuantity] = useState("");
  const [price, setPrice] = useState("");
  const [brokerage, setBrokerage] = useState("0");
  const [stt, setStt] = useState("0");
  const [otherCharges, setOtherCharges] = useState("0");
  const [tds, setTds] = useState("0");
  const [notes, setNotes] = useState("");
  const [brokerRef, setBrokerRef] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  // Load person/portfolio/account data when dialog opens
  useEffect(() => {
    if (open) {
      apiGet<Person[]>("/persons").then(setPersons).catch(() => {});
      apiGet<Portfolio[]>("/portfolios").then(setPortfolios).catch(() => {});
      apiGet<Account[]>("/accounts").then(setAllAccounts).catch(() => {});
    }
  }, [open]);

  // Derive asset class from the selected instrument
  const assetClass = useMemo(() => {
    if (!instrument) return "EQUITY";
    // New pending: type comes from the pending_instrument spec
    if (instrument.pending_instrument) return instrument.pending_instrument.type;
    return instrument.asset_class || "EQUITY";
  }, [instrument]);

  const classConfig = useMemo(() => getClassConfig(assetClass), [assetClass]);

  const availableTxnTypes = useMemo(
    () => TXN_TYPES.filter(t => classConfig.availableTypes.includes(t.value)),
    [classConfig],
  );

  // When instrument changes: reset txnType to first valid for new class; force segment
  useEffect(() => {
    if (!classConfig.availableTypes.includes(txnType)) {
      setTxnType(classConfig.availableTypes[0] ?? "BUY");
    }
    if (classConfig.forcedSegment) {
      setSegment(classConfig.forcedSegment);
    }
  }, [assetClass]); // eslint-disable-line react-hooks/exhaustive-deps

  const selectedType = TXN_TYPES.find(t => t.value === txnType) ?? TXN_TYPES[0];

  // When txnType changes, update segment (unless forced by asset class)
  useEffect(() => {
    if (!classConfig.forcedSegment) {
      setSegment(selectedType.defaultSegment);
    }
  }, [txnType]); // eslint-disable-line react-hooks/exhaustive-deps

  const showCharges = !classConfig.hideCharges && selectedType.showCharges;

  const totalPaise = (() => {
    const gross   = parseFloat(quantity || "0") * rupeesToPaise(price);
    const charges = rupeesToPaise(brokerage) + rupeesToPaise(stt) + rupeesToPaise(otherCharges);
    if (["BUY", "SIP", "IPO", "FPO", "OPENING_BALANCE", "TRANSFER_IN", "MERGER_IN", "SWITCH_IN"].includes(txnType))
      return -(gross + charges);
    if (["SELL", "REDEMPTION", "TRANSFER_OUT", "MERGER_OUT", "SWITCH_OUT"].includes(txnType))
      return gross - charges;
    return gross;
  })();

  const reset = () => {
    setSelection({});
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
    setTds("0");
    setNotes("");
    setBrokerRef("");
    setError("");
  };

  const handleSave = async () => {
    if (!accountId)  { setError("Select an account"); return; }
    if (!instrument) { setError("Select an instrument"); return; }
    if (!quantity || parseFloat(quantity) <= 0) { setError("Enter a valid quantity"); return; }
    if (selectedType.showPrice && (!price || rupeesToPaise(price) <= 0)) { setError("Enter a valid price"); return; }

    setSaving(true);
    setError("");
    try {
      const isResolved        = instrument.instrument_id > 0;
      const isExistingPending = instrument.pending_instrument_id != null;
      const isNewPending      = !isResolved && !isExistingPending && !!instrument.pending_instrument;
      const txn = await apiPost<{ txn_id: number }>("/transactions", {
        account_id:          parseInt(accountId),
        instrument_id:       isResolved        ? instrument.instrument_id         : null,
        existing_pending_id: isExistingPending ? instrument.pending_instrument_id : null,
        pending_instrument:  isNewPending      ? instrument.pending_instrument    : null,
        txn_type:            txnType,
        trade_segment:       segment,
        trade_date:          tradeDate,
        txn_time:            txnTime || null,
        quantity:            parseFloat(quantity),
        effective_price_paise: rupeesToPaise(price),
        brokerage_per_unit_paise: rupeesToPaise(brokerage) > 0 ? Math.round(rupeesToPaise(brokerage) / parseFloat(quantity)) : null,
        actual_price_paise: null,
        stt_paise:           rupeesToPaise(stt),
        other_charges_paise: rupeesToPaise(otherCharges),
        notes:               notes || null,
        broker_ref:          brokerRef || null,
      });

      const tdsPaise = rupeesToPaise(tds);
      const personId = selection.person?.person_id;
      if (tdsPaise > 0 && personId) {
        await apiPost("/tax", {
          person_id:    personId,
          entry_type:   "TDS",
          amount_paise: tdsPaise,
          entry_date:   tradeDate,
          fy:           dateToFY(tradeDate),
          txn_id:       txn.txn_id,
          notes:        `TDS on ${txnType} — ${instrument.name ?? ""}`.trim(),
        });
      }

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
          {/* Account — cascading person → portfolio → account */}
          <div className="border rounded-lg p-3 space-y-1">
            <PersonPortfolioAccountSelector
              persons={persons}
              portfolios={portfolios}
              accounts={allAccounts}
              value={selection}
              onChange={setSelection}
              onPersonCreated={(p) => setPersons(ps => [...ps, p])}
              onPortfolioCreated={(p) => setPortfolios(ps => [...ps, p])}
              onAccountCreated={(a) => setAllAccounts(as => [...as, a])}
              showAccount={true}
            />
          </div>

          {/* Instrument */}
          <div className="space-y-1.5">
            <Label>Instrument</Label>
            <InstrumentSearch value={instrument ?? undefined} onChange={setInstrument} />
          </div>

          {/* Type + Segment */}
          <div className={`grid gap-3 ${classConfig.showSegment ? "grid-cols-2" : "grid-cols-1"}`}>
            <div className="space-y-1.5">
              <Label>Transaction type</Label>
              <Select value={txnType} onValueChange={(v) => v && setTxnType(v)}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  {availableTxnTypes.map((t) => (
                    <SelectItem key={t.value} value={t.value}>{t.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            {classConfig.showSegment && (
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
            )}
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
              <Label>{classConfig.quantityLabel}</Label>
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
                <Label>{classConfig.priceLabel}</Label>
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

          {/* Charges — hidden for MF, controlled by txn type for others */}
          {showCharges && (
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

          {/* TDS — shown only for income txn types */}
          {(txnType === "DIVIDEND" || txnType === "INTEREST") && (
            <div className="space-y-1.5">
              <Label className="text-xs">
                TDS deducted (₹) <span className="text-muted-foreground">(optional)</span>
              </Label>
              <Input
                type="number"
                min="0"
                step="0.01"
                placeholder="0.00"
                value={tds}
                onChange={(e) => setTds(e.target.value)}
              />
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
