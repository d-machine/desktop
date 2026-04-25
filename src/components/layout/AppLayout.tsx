import { useState } from "react";
import { AppNavbar } from "./AppNavbar";
import { type Page } from "@/lib/nav";

// Pages
import { DashboardPage } from "@/pages/DashboardPage";
import { HoldingsPage } from "@/pages/HoldingsPage";
import { TransactionsPage } from "@/pages/TransactionsPage";
import { CapitalGainsPage } from "@/pages/CapitalGainsPage";
import { IncomePage } from "@/pages/IncomePage";
import { AssetAllocationPage } from "@/pages/AssetAllocationPage";
import { ReportsPage } from "@/pages/ReportsPage";
import { SettingsPage } from "@/pages/SettingsPage";

interface AppLayoutProps {
  onLock: () => void;
}

function PageContent({
  page,
  instrumentId,
  onNavigate,
}: {
  page: Page;
  instrumentId: number | undefined;
  onNavigate: (page: Page, instrumentId?: number) => void;
}) {
  switch (page) {
    case "dashboard":        return <DashboardPage onNavigate={onNavigate} />;
    case "holdings":         return <HoldingsPage initialInstrumentId={instrumentId} />;
    case "transactions":     return <TransactionsPage />;
    case "capital-gains":    return <CapitalGainsPage />;
    case "income":           return <IncomePage />;
    case "asset-allocation": return <AssetAllocationPage />;
    case "reports":          return <ReportsPage />;
    case "settings":         return <SettingsPage />;
  }
}

export function AppLayout({ onLock }: AppLayoutProps) {
  const [currentPage, setCurrentPage]       = useState<Page>("dashboard");
  const [navInstrumentId, setNavInstrumentId] = useState<number | undefined>();

  const navigate = (page: Page, instrumentId?: number) => {
    setCurrentPage(page);
    setNavInstrumentId(instrumentId);
  };

  return (
    <div className="flex flex-col h-screen w-full overflow-hidden bg-background">
      <AppNavbar
        currentPage={currentPage}
        onNavigate={(page) => navigate(page)}
        onLock={onLock}
      />
      <main className="flex-1 overflow-y-auto p-6">
        <PageContent page={currentPage} instrumentId={navInstrumentId} onNavigate={navigate} />
      </main>
    </div>
  );
}
