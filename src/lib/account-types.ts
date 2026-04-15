export interface AccountTypeOption {
  value: string;
  label: string;
  description: string;
  brokerRequired: boolean;
}

export const ACCOUNT_TYPES: AccountTypeOption[] = [
  { value: "DEMAT",     label: "Demat Account",    description: "Equity, F&O trading",         brokerRequired: true  },
  { value: "MF_FOLIO",  label: "MF Folio",         description: "Mutual fund investments",      brokerRequired: false },
  { value: "FD",        label: "Fixed Deposit",    description: "Bank FD, corporate deposits",  brokerRequired: false },
  { value: "PPF",       label: "PPF",              description: "Public Provident Fund",         brokerRequired: false },
  { value: "NPS",       label: "NPS",              description: "National Pension System",       brokerRequired: false },
  { value: "OTHER",     label: "Other",            description: "Gold, unlisted shares, etc.",  brokerRequired: false },
];

export const BROKERS = [
  "Zerodha", "Groww", "Upstox", "Angel One", "HDFC Securities",
  "ICICI Direct", "Kotak Securities", "Motilal Oswal", "SBI Securities",
  "Paytm Money", "5Paisa", "Fyers", "Other",
];
