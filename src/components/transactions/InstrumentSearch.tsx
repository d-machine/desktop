import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Search, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { PendingInstrumentForm, PendingInstrumentSpec, PendingInstrumentInitialValues, AssetClass } from "./PendingInstrumentForm";

export interface InstrumentSummary {
  instrument_id: number;      // positive = resolved, negative = pending (-pending_id)
  isin?: string;
  name: string;
  asset_class: string;
  exchange_code?: string;
  nse_symbol?: string;
  pending_instrument?: PendingInstrumentSpec;    // set when creating a new pending
  pending_instrument_id?: number;               // set when linking an existing pending
}

interface InstrumentSearchProps {
  value?: InstrumentSummary;
  onChange: (instrument: InstrumentSummary) => void;
  placeholder?: string;
  disabled?: boolean;
}

/** Parse `pending_metadata` JSON from the server into PendingInstrumentForm's field map. */
function parseInitialValues(name: string, assetClass: string, metadataJson?: string): PendingInstrumentInitialValues {
  const type = (assetClass as AssetClass) ?? "EQUITY";
  let fields: Record<string, string> = { name };
  if (metadataJson) {
    try {
      const meta = JSON.parse(metadataJson) as Record<string, unknown>;
      for (const [k, v] of Object.entries(meta)) {
        if (v != null) fields[k] = String(v);
      }
    } catch { /* ignore malformed JSON */ }
  }
  return { name, type, fields };
}

export function InstrumentSearch({ value, onChange, placeholder = "Search by name, ISIN or symbol…", disabled }: InstrumentSearchProps) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<(InstrumentSummary & { pending_instrument_id?: number; pending_metadata?: string })[]>([]);
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [showManual, setShowManual] = useState(false);
  const [pendingInitial, setPendingInitial] = useState<PendingInstrumentInitialValues | undefined>(undefined);
  const [existingPendingId, setExistingPendingId] = useState<number | undefined>(undefined);
  const containerRef = useRef<HTMLDivElement>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    if (query.length < 2) { setResults([]); setOpen(false); return; }
    clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(async () => {
      setLoading(true);
      try {
        const r = await invoke<(InstrumentSummary & { pending_instrument_id?: number; pending_metadata?: string })[]>(
          "search_instruments", { query }
        );
        setResults(r);
        setOpen(r.length > 0);
      } finally {
        setLoading(false);
      }
    }, 300);
    return () => clearTimeout(debounceRef.current);
  }, [query]);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, []);

  const selectResolved = (instrument: InstrumentSummary) => {
    onChange(instrument);
    setQuery("");
    setOpen(false);
    setShowManual(false);
    setPendingInitial(undefined);
    setExistingPendingId(undefined);
  };

  const selectExistingPending = (r: InstrumentSummary & { pending_instrument_id?: number; pending_metadata?: string }) => {
    setOpen(false);
    setExistingPendingId(r.pending_instrument_id);
    setPendingInitial(parseInitialValues(r.name, r.asset_class, r.pending_metadata));
    setShowManual(true);
  };

  const clear = () => {
    onChange(null as any);
    setQuery("");
    setShowManual(false);
    setPendingInitial(undefined);
    setExistingPendingId(undefined);
  };

  const handleManualConfirm = async (spec: PendingInstrumentSpec) => {
    if (existingPendingId != null) {
      // Enrich existing pending instrument's metadata in-place
      await invoke("update_pending_instrument", {
        pendingId: existingPendingId,
        name: spec.name,
        metadata: spec.metadata,
      }).catch(() => { /* non-fatal — transaction still links correctly */ });

      selectResolved({
        instrument_id:        -existingPendingId,
        name:                 spec.name,
        asset_class:          spec.type,
        pending_instrument_id: existingPendingId,
      });
    } else {
      // New pending instrument — created on save by create_transaction
      selectResolved({
        instrument_id:      -1,
        name:               spec.name,
        asset_class:        spec.type,
        pending_instrument: spec,
      });
    }
  };

  if (value) {
    return (
      <div className="flex items-center gap-2 border rounded-md px-3 py-2 bg-muted/30">
        <div className="flex-1 min-w-0">
          <div className="text-sm font-medium truncate">{value.name}</div>
          <div className="text-xs text-muted-foreground">
            {value.isin && <span className="mr-2">{value.isin}</span>}
            {value.nse_symbol && <span className="mr-2">{value.nse_symbol}</span>}
            <span className={cn(
              "px-1.5 py-0.5 rounded text-xs",
              (value.pending_instrument || value.pending_instrument_id != null) &&
                "bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400",
            )}>
              {(value.pending_instrument || value.pending_instrument_id != null) ? "PENDING" : value.asset_class}
            </span>
          </div>
        </div>
        {!disabled && (
          <button onClick={clear} className="text-muted-foreground hover:text-foreground shrink-0">
            <X className="size-4" />
          </button>
        )}
      </div>
    );
  }

  if (showManual) {
    return (
      <PendingInstrumentForm
        initialValues={pendingInitial}
        allowSkip={existingPendingId != null}
        onConfirm={handleManualConfirm}
        onCancel={() => { setShowManual(false); setPendingInitial(undefined); setExistingPendingId(undefined); }}
      />
    );
  }

  return (
    <div ref={containerRef} className="relative">
      <div className="relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
        <input
          className="w-full pl-9 pr-3 py-2 text-sm border rounded-md bg-background focus:outline-none focus:ring-2 focus:ring-ring"
          placeholder={placeholder}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          disabled={disabled}
        />
        {loading && (
          <div className="absolute right-3 top-1/2 -translate-y-1/2 size-4 border-2 border-muted-foreground border-t-transparent rounded-full animate-spin" />
        )}
      </div>

      {open && (
        <div className="absolute z-50 top-full mt-1 w-full bg-popover border rounded-md shadow-md overflow-hidden">
          <div className="max-h-60 overflow-y-auto">
            {results.map((r) => {
              const isPending = r.pending_instrument_id != null;
              return (
                <button
                  key={r.instrument_id}
                  className="w-full text-left px-3 py-2.5 hover:bg-accent transition-colors"
                  onClick={() => isPending ? selectExistingPending(r) : selectResolved(r)}
                >
                  <div className="text-sm font-medium">{r.name}</div>
                  <div className="text-xs text-muted-foreground">
                    {r.isin && <span className="mr-2">{r.isin}</span>}
                    {r.nse_symbol && <span className="mr-2 font-mono">{r.nse_symbol}</span>}
                    <span className={cn(
                      "px-1.5 py-0.5 rounded text-xs",
                      isPending
                        ? "bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400"
                        : r.asset_class === "EQUITY"       ? "bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400"
                        : r.asset_class === "MF"           ? "bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400"
                        : r.asset_class === "FIXED_INCOME" ? "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400"
                        : ""
                    )}>
                      {isPending ? "PENDING" : r.asset_class}
                    </span>
                  </div>
                </button>
              );
            })}
          </div>
          <div className="border-t px-3 py-2">
            <button
              className="text-xs text-muted-foreground hover:text-foreground"
              onClick={() => { setOpen(false); setPendingInitial(undefined); setExistingPendingId(undefined); setShowManual(true); }}
            >
              + Not found? Add manually
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
