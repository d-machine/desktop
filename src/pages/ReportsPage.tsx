import { useState } from "react";
import { cn } from "@/lib/utils";
import { CapitalGainsPage } from "@/pages/CapitalGainsPage";
import { ChargesTab } from "@/components/reports/ChargesTab";

type ReportTab = "capital-gains" | "charges";

const TABS: { id: ReportTab; label: string }[] = [
  { id: "capital-gains", label: "Capital Gains" },
  { id: "charges",       label: "Charges" },
];

export function ReportsPage({ personAccountIds }: { personAccountIds?: number[] | null }) {
  const [tab, setTab] = useState<ReportTab>("capital-gains");

  return (
    <div className="flex flex-col gap-4 h-full">
      {/* Tab bar */}
      <div className="flex gap-1 border-b pb-0">
        {TABS.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={cn(
              "px-4 py-2 text-sm font-medium border-b-2 -mb-px transition-colors",
              tab === t.id
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground"
            )}
          >
            {t.label}
          </button>
        ))}
      </div>

      {/* Tab content */}
      <div className="flex-1 min-h-0">
        {tab === "capital-gains" && <CapitalGainsPage personAccountIds={personAccountIds} />}
        {tab === "charges"       && <ChargesTab personAccountIds={personAccountIds} />}
      </div>
    </div>
  );
}
