export interface TxnTypeOption {
  value: string;
  label: string;
  defaultSegment: string;
  showPrice: boolean;
  showCharges: boolean;
}

export const TXN_TYPES: TxnTypeOption[] = [
  { value: "BUY",            label: "Buy",             defaultSegment: "DELIVERY",  showPrice: true,  showCharges: true  },
  { value: "SELL",           label: "Sell",            defaultSegment: "DELIVERY",  showPrice: true,  showCharges: true  },
  { value: "SIP",            label: "SIP",             defaultSegment: "DELIVERY",  showPrice: true,  showCharges: false },
  { value: "REDEMPTION",     label: "Redemption",      defaultSegment: "DELIVERY",  showPrice: true,  showCharges: false },
  { value: "DIVIDEND",       label: "Dividend",        defaultSegment: "DELIVERY",  showPrice: true,  showCharges: false },
  { value: "INTEREST",       label: "Interest",        defaultSegment: "DELIVERY",  showPrice: true,  showCharges: false },
  { value: "BONUS",          label: "Bonus",           defaultSegment: "DELIVERY",  showPrice: false, showCharges: false },
  { value: "SPLIT",          label: "Stock Split",     defaultSegment: "DELIVERY",  showPrice: false, showCharges: false },
  { value: "OPENING_BALANCE",label: "Opening Balance", defaultSegment: "DELIVERY",  showPrice: true,  showCharges: false },
  { value: "TRANSFER_IN",    label: "Transfer In",     defaultSegment: "DELIVERY",  showPrice: true,  showCharges: false },
  { value: "TRANSFER_OUT",   label: "Transfer Out",    defaultSegment: "DELIVERY",  showPrice: true,  showCharges: false },
  { value: "MERGER_IN",      label: "Merger In",       defaultSegment: "DELIVERY",  showPrice: true,  showCharges: false },
  { value: "MERGER_OUT",     label: "Merger Out",      defaultSegment: "DELIVERY",  showPrice: true,  showCharges: false },
  { value: "SWITCH_IN",      label: "Switch In",       defaultSegment: "DELIVERY",  showPrice: true,  showCharges: false },
  { value: "SWITCH_OUT",     label: "Switch Out",      defaultSegment: "DELIVERY",  showPrice: true,  showCharges: false },
];

export const TRADE_SEGMENTS = [
  { value: "DELIVERY",  label: "Delivery" },
  { value: "INTRADAY",  label: "Intraday" },
  { value: "FNO",       label: "F&O" },
  { value: "COMMODITY", label: "Commodity" },
];

export const TXN_TYPE_COLORS: Record<string, string> = {
  BUY:             "text-blue-600 dark:text-blue-400",
  SIP:             "text-blue-600 dark:text-blue-400",
  SELL:            "text-red-600 dark:text-red-400",
  REDEMPTION:      "text-red-600 dark:text-red-400",
  DIVIDEND:        "text-green-600 dark:text-green-400",
  INTEREST:        "text-green-600 dark:text-green-400",
  BONUS:           "text-purple-600 dark:text-purple-400",
  SPLIT:           "text-purple-600 dark:text-purple-400",
  OPENING_BALANCE: "text-muted-foreground",
  TRANSFER_IN:     "text-blue-600 dark:text-blue-400",
  TRANSFER_OUT:    "text-orange-600 dark:text-orange-400",
  MERGER_IN:       "text-purple-600 dark:text-purple-400",
  MERGER_OUT:      "text-purple-600 dark:text-purple-400",
  SWITCH_IN:       "text-teal-600 dark:text-teal-400",
  SWITCH_OUT:      "text-teal-600 dark:text-teal-400",
};
