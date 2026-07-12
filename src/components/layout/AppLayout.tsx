import { useState, useEffect } from "react";
import { AppNavbar } from "./AppNavbar";
import { type Page } from "@/lib/nav";
import { type ServerAuthState } from "@/components/auth/ServerLoginDialog";
import { type PersonRecord } from "@/App";
import { apiGet } from "@/lib/api";

// Pages
import { DashboardPage } from "@/pages/DashboardPage";
import { HoldingsPage } from "@/pages/HoldingsPage";
import { TransactionsPage } from "@/pages/TransactionsPage";
import { IncomePage } from "@/pages/IncomePage";
import { AssetAllocationPage } from "@/pages/AssetAllocationPage";
import { ReportsPage } from "@/pages/ReportsPage";
import { SettingsPage } from "@/pages/SettingsPage";
import { TaxPage } from "@/pages/TaxPage";

interface Portfolio { portfolio_id: number; person_id: number | null; }
interface Account   { account_id: number; portfolio_id: number; }

interface AppLayoutProps {
  onLock: () => void;
  serverAuth: ServerAuthState;
  onServerAuthChange: (state: ServerAuthState) => void;
  activePerson: PersonRecord | null;
  persons: PersonRecord[];
  onPersonChange: (p: PersonRecord) => void;
}

function PageContent({
  page,
  instrumentId,
  onNavigate,
  activePerson,
  personPortfolioIds,
  personAccountIds,
}: {
  page: Page;
  instrumentId: number | undefined;
  onNavigate: (page: Page, instrumentId?: number) => void;
  activePerson: PersonRecord | null;
  personPortfolioIds: number[] | null;
  personAccountIds: number[] | null;
}) {
  switch (page) {
    case "dashboard":        return <DashboardPage onNavigate={onNavigate} personPortfolioIds={personPortfolioIds} personAccountIds={personAccountIds} />;
    case "holdings":         return <HoldingsPage initialInstrumentId={instrumentId} personPortfolioIds={personPortfolioIds} />;
    case "transactions":     return <TransactionsPage activePerson={activePerson} personPortfolioIds={personPortfolioIds} personAccountIds={personAccountIds} />;
    case "income":           return <IncomePage personAccountIds={personAccountIds} />;
    case "asset-allocation": return <AssetAllocationPage />;
    case "reports":          return <ReportsPage personAccountIds={personAccountIds} />;
    case "tax":              return <TaxPage activePerson={activePerson} personAccountIds={personAccountIds} />;
    case "settings":         return <SettingsPage />;
  }
}

export function AppLayout({ onLock, serverAuth, onServerAuthChange, activePerson, persons, onPersonChange }: AppLayoutProps) {
  const [currentPage, setCurrentPage]           = useState<Page>("dashboard");
  const [navInstrumentId, setNavInstrumentId]   = useState<number | undefined>();
  const [allPortfolios, setAllPortfolios]        = useState<Portfolio[]>([]);
  const [allAccounts, setAllAccounts]            = useState<Account[]>([]);
  const [filtersReady, setFiltersReady]          = useState(false);

  useEffect(() => {
    Promise.all([
      apiGet<Portfolio[]>("/portfolios"),
      apiGet<Account[]>("/accounts"),
    ]).then(([ps, as]) => {
      setAllPortfolios(ps);
      setAllAccounts(as);
      setFiltersReady(true);
    }).catch(() => { setFiltersReady(true); });
  }, []);

  const personPortfolioIds: number[] | null = activePerson
    ? (() => {
        const ids = allPortfolios
          .filter(p => p.person_id === activePerson.person_id)
          .map(p => p.portfolio_id);
        return ids.length > 0 ? ids : [-1];
      })()
    : null;

  const personAccountIds: number[] | null = personPortfolioIds
    ? (() => {
        if (personPortfolioIds[0] === -1) return [-1];
        const ids = allAccounts.filter(a => personPortfolioIds.includes(a.portfolio_id)).map(a => a.account_id);
        return ids.length > 0 ? ids : [-1];
      })()
    : null;

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
        serverAuth={serverAuth}
        onServerAuthChange={onServerAuthChange}
        activePerson={activePerson}
        persons={persons}
        onPersonChange={onPersonChange}
      />
      <main className="flex-1 overflow-y-auto p-6">
        {filtersReady && (
          <PageContent
            key={activePerson?.person_id ?? "none"}
            page={currentPage}
            instrumentId={navInstrumentId}
            onNavigate={navigate}
            activePerson={activePerson}
            personPortfolioIds={personPortfolioIds}
            personAccountIds={personAccountIds}
          />
        )}
      </main>
    </div>
  );
}
