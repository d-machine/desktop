import {
  LayoutDashboard,
  Briefcase,
  ArrowLeftRight,
  TrendingUp,
  PieChart,
  Receipt,
  FileText,
  Settings,
  type LucideIcon,
} from "lucide-react";

export interface NavItem {
  label: string;
  page: Page;
  icon: LucideIcon;
  group: "main" | "reports" | "settings";
}

export type Page =
  | "dashboard"
  | "holdings"
  | "transactions"
  | "capital-gains"
  | "income"
  | "asset-allocation"
  | "reports"
  | "settings";

export const NAV_ITEMS: NavItem[] = [
  { label: "Dashboard",      page: "dashboard",        icon: LayoutDashboard, group: "main" },
  { label: "Holdings",       page: "holdings",         icon: Briefcase,       group: "main" },
  { label: "Transactions",   page: "transactions",     icon: ArrowLeftRight,  group: "main" },
  { label: "Capital Gains",  page: "capital-gains",    icon: TrendingUp,      group: "reports" },
  { label: "Income",         page: "income",           icon: Receipt,         group: "reports" },
  { label: "Allocation",     page: "asset-allocation", icon: PieChart,        group: "reports" },
  { label: "Reports",        page: "reports",          icon: FileText,        group: "reports" },
  { label: "Settings",       page: "settings",         icon: Settings,        group: "settings" },
];
