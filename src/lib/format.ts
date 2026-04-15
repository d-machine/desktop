/** Format paise (integer) as ₹ string with 2 decimal places */
export function formatINR(paise: number): string {
  const rupees = paise / 100;
  return new Intl.NumberFormat("en-IN", {
    style: "currency",
    currency: "INR",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(rupees);
}

/** Format a quantity — MFs have up to 3 decimal places, equity is whole */
export function formatQty(qty: number): string {
  if (Number.isInteger(qty)) return qty.toLocaleString("en-IN");
  return qty.toLocaleString("en-IN", { maximumFractionDigits: 3 });
}

/** Format ISO date string to DD MMM YYYY */
export function formatDate(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleDateString("en-IN", { day: "2-digit", month: "short", year: "numeric" });
}

/** Format paise as rupees string without currency symbol */
export function paiseToRupees(paise: number): number {
  return paise / 100;
}

/** Convert rupees string input to paise integer */
export function rupeesToPaise(rupees: string): number {
  const val = parseFloat(rupees.replace(/,/g, ""));
  if (isNaN(val)) return 0;
  return Math.round(val * 100);
}
