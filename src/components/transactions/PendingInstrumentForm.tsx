import { useState } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";

export type AssetClass = "EQUITY" | "MF" | "FUTSTK" | "FUTIDX" | "OPTSTK" | "OPTIDX" | "MCX";

export interface PendingInstrumentSpec {
  name: string;
  type: AssetClass;
  metadata: Record<string, string | number | null>;
}

export interface PendingInstrumentInitialValues {
  name: string;
  type: AssetClass;
  fields: Record<string, string>;
}

interface PendingInstrumentFormProps {
  onConfirm: (spec: PendingInstrumentSpec) => void;
  onCancel: () => void;
  initialValues?: PendingInstrumentInitialValues;
  /** When true, show a "Skip" button that confirms without requiring all fields */
  allowSkip?: boolean;
}

const ASSET_CLASSES: { value: AssetClass; label: string }[] = [
  { value: "EQUITY",  label: "Equity / ETF" },
  { value: "MF",      label: "Mutual Fund" },
  { value: "FUTSTK",  label: "Stock Future" },
  { value: "FUTIDX",  label: "Index Future" },
  { value: "OPTSTK",  label: "Stock Option" },
  { value: "OPTIDX",  label: "Index Option" },
  { value: "MCX",     label: "MCX Commodity" },
];

function buildName(type: AssetClass, f: Record<string, string>): string {
  switch (type) {
    case "FUTSTK": return f.underlying && f.expiry ? `FUTSTK ${f.underlying} ${f.expiry}` : "";
    case "FUTIDX": return f.underlying && f.expiry ? `FUTIDX ${f.underlying} ${f.expiry}` : "";
    case "OPTSTK": return f.underlying && f.expiry && f.strike && f.option_type
      ? `OPTSTK ${f.underlying} ${f.expiry} ${f.strike} ${f.option_type}` : "";
    case "OPTIDX": return f.underlying && f.expiry && f.strike && f.option_type
      ? `OPTIDX ${f.underlying} ${f.expiry} ${f.strike} ${f.option_type}` : "";
    default: return f.name ?? "";
  }
}

export function PendingInstrumentForm({ onConfirm, onCancel, initialValues, allowSkip }: PendingInstrumentFormProps) {
  const [type, setType] = useState<AssetClass>(initialValues?.type ?? "EQUITY");
  const [fields, setFields] = useState<Record<string, string>>(initialValues?.fields ?? {});

  const set = (key: string, val: string) =>
    setFields(prev => ({ ...prev, [key]: val }));

  const computedName = buildName(type, fields);
  const displayName  = computedName || fields.name || "";

  const handleConfirm = () => {
    if (!displayName) return;

    const metadata: Record<string, string | number | null> = {};
    const addStr = (k: string) => { if (fields[k]) metadata[k] = fields[k]; };
    const addNum = (k: string) => {
      const v = parseFloat(fields[k]);
      if (!isNaN(v)) metadata[k] = v;
    };

    switch (type) {
      case "EQUITY":
        addStr("isin"); addStr("nse_symbol"); addStr("bse_code"); addStr("exchange");
        break;
      case "MF":
        addStr("isin"); addStr("amfi_code"); addStr("amc");
        break;
      case "FUTSTK":
      case "FUTIDX":
        addStr("underlying_symbol"); addStr("expiry_date"); addStr("exchange");
        break;
      case "OPTSTK":
      case "OPTIDX":
        addStr("underlying_symbol"); addStr("expiry_date"); addStr("option_type"); addStr("exchange");
        addNum("strike_price_paise");
        break;
      case "MCX":
        addStr("mcx_symbol"); addStr("expiry_date"); addStr("unit");
        break;
    }

    onConfirm({ name: displayName, type, metadata });
  };

  const isValid = !!displayName;

  return (
    <div className="border rounded-md p-4 space-y-3 bg-muted/20">
      <div className="text-sm font-medium">Add instrument manually</div>

      {/* Asset class */}
      <div className="space-y-1.5">
        <Label className="text-xs">Asset class</Label>
        <Select value={type} onValueChange={(v) => { if (v) { setType(v as AssetClass); setFields({}); } }}>
          <SelectTrigger className="h-8 text-xs"><SelectValue /></SelectTrigger>
          <SelectContent>
            {ASSET_CLASSES.map(c => (
              <SelectItem key={c.value} value={c.value} className="text-xs">{c.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* Equity */}
      {type === "EQUITY" && (
        <>
          <Field label="Name" value={fields.name ?? ""} onChange={v => set("name", v)} placeholder="Punjab National Bank" />
          <div className="grid grid-cols-2 gap-2">
            <Field label="ISIN" value={fields.isin ?? ""} onChange={v => set("isin", v)} placeholder="INE160A01022" />
            <Field label="NSE Symbol" value={fields.nse_symbol ?? ""} onChange={v => set("nse_symbol", v)} placeholder="PNB" />
          </div>
          <div className="grid grid-cols-2 gap-2">
            <Field label="BSE Code" value={fields.bse_code ?? ""} onChange={v => set("bse_code", v)} placeholder="532461" />
            <ExchangeSelect value={fields.exchange ?? "NSE"} onChange={v => set("exchange", v)} />
          </div>
        </>
      )}

      {/* MF */}
      {type === "MF" && (
        <>
          <Field label="Scheme name" value={fields.name ?? ""} onChange={v => set("name", v)} placeholder="Parag Parikh Flexi Cap Fund Direct Growth" />
          <div className="grid grid-cols-2 gap-2">
            <Field label="ISIN" value={fields.isin ?? ""} onChange={v => set("isin", v)} placeholder="INF879O01019" />
            <Field label="AMFI code" value={fields.amfi_code ?? ""} onChange={v => set("amfi_code", v)} placeholder="122639" />
          </div>
          <Field label="AMC" value={fields.amc ?? ""} onChange={v => set("amc", v)} placeholder="PPFAS Mutual Fund" />
        </>
      )}

      {/* FUTSTK / FUTIDX */}
      {(type === "FUTSTK" || type === "FUTIDX") && (
        <>
          <div className="grid grid-cols-2 gap-2">
            <Field label={type === "FUTIDX" ? "Index" : "Underlying"} value={fields.underlying_symbol ?? ""} onChange={v => set("underlying_symbol", v)} placeholder={type === "FUTIDX" ? "NIFTY" : "RELIANCE"} />
            <ExchangeSelect value={fields.exchange ?? "NSE"} onChange={v => set("exchange", v)} />
          </div>
          <Field label="Expiry date" value={fields.expiry_date ?? ""} onChange={v => set("expiry_date", v)} placeholder="2026-04-28" type="date" />
          {computedName && <Preview name={computedName} />}
        </>
      )}

      {/* OPTSTK / OPTIDX */}
      {(type === "OPTSTK" || type === "OPTIDX") && (
        <>
          <div className="grid grid-cols-2 gap-2">
            <Field label={type === "OPTIDX" ? "Index" : "Underlying"} value={fields.underlying_symbol ?? ""} onChange={v => set("underlying_symbol", v)} placeholder={type === "OPTIDX" ? "NIFTY" : "RELIANCE"} />
            <ExchangeSelect value={fields.exchange ?? "NSE"} onChange={v => set("exchange", v)} />
          </div>
          <div className="grid grid-cols-3 gap-2">
            <Field label="Expiry date" value={fields.expiry_date ?? ""} onChange={v => set("expiry_date", v)} placeholder="2026-04-28" type="date" />
            <Field label="Strike (₹)" value={fields.strike_price_paise ?? ""} onChange={v => set("strike_price_paise", v)} placeholder="22000" type="number" />
            <div className="space-y-1.5">
              <Label className="text-xs">Type</Label>
              <Select value={fields.option_type ?? "CE"} onValueChange={v => set("option_type", v ?? "CE")}>
                <SelectTrigger className="h-8 text-xs"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="CE" className="text-xs">CE — Call</SelectItem>
                  <SelectItem value="PE" className="text-xs">PE — Put</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          {computedName && <Preview name={computedName} />}
        </>
      )}

      {/* MCX */}
      {type === "MCX" && (
        <>
          <div className="grid grid-cols-2 gap-2">
            <Field label="Commodity" value={fields.mcx_symbol ?? ""} onChange={v => set("mcx_symbol", v)} placeholder="GOLD" />
            <Field label="Unit" value={fields.unit ?? ""} onChange={v => set("unit", v)} placeholder="GRAM" />
          </div>
          <Field label="Expiry date" value={fields.expiry_date ?? ""} onChange={v => set("expiry_date", v)} placeholder="2026-04-28" type="date" />
        </>
      )}

      <div className="flex gap-2 pt-1">
        <Button size="sm" onClick={handleConfirm} disabled={!isValid} className="text-xs">
          Use this instrument
        </Button>
        {allowSkip && (
          <Button size="sm" variant="outline" onClick={() => onConfirm({ name: displayName, type, metadata: {} })} disabled={!displayName} className="text-xs">
            Skip enrichment
          </Button>
        )}
        <Button size="sm" variant="ghost" onClick={onCancel} className="text-xs">
          Cancel
        </Button>
      </div>
    </div>
  );
}

function Field({ label, value, onChange, placeholder, type = "text" }: {
  label: string; value: string; onChange: (v: string) => void; placeholder?: string; type?: string;
}) {
  return (
    <div className="space-y-1.5">
      <Label className="text-xs">{label}</Label>
      <Input className="h-8 text-xs" type={type} value={value} onChange={e => onChange(e.target.value)} placeholder={placeholder} />
    </div>
  );
}

function ExchangeSelect({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="space-y-1.5">
      <Label className="text-xs">Exchange</Label>
      <Select value={value} onValueChange={v => onChange(v ?? "NSE")}>
        <SelectTrigger className="h-8 text-xs"><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem value="NSE" className="text-xs">NSE</SelectItem>
          <SelectItem value="BSE" className="text-xs">BSE</SelectItem>
          <SelectItem value="MCX" className="text-xs">MCX</SelectItem>
        </SelectContent>
      </Select>
    </div>
  );
}

function Preview({ name }: { name: string }) {
  return (
    <div className="text-xs text-muted-foreground bg-muted/30 rounded px-2 py-1 font-mono">
      {name}
    </div>
  );
}
