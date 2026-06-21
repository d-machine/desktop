import sys, os
sys.path.insert(0, ".")
from importers.bajaj_finance import parse

folder = r"C:\Users\SUMIT\Desktop\harsh\APRIL-26"
password = "GYEPS4368P"
total = 0
all_trades = []

for fname in sorted(os.listdir(folder)):
    if not fname.endswith(".pdf"):
        continue
    path = os.path.join(folder, fname)
    try:
        result = parse(path, password=password)
        trades = result["trades"]
        total += len(trades)
        cn = result["contract_note_no"]
        print(f"{fname}: {len(trades)} trades  CN={cn}  date={result['trade_date']}")
        for t in trades:
            key = f"{cn}-{t['isin']}-{t['side']}"
            print(f"    {t['side']:4}  {t['quantity']:8,}  {t['isin']}  {t['name'][:28]}  broker_ref={key}")
            all_trades.append(key)
    except Exception as e:
        print(f"{fname}: ERROR — {e}")

print(f"\nTotal trades parsed:  {total}")
print(f"Unique broker_refs:   {len(set(all_trades))}")

dupes = [k for k in all_trades if all_trades.count(k) > 1]
if dupes:
    print(f"\nDUPLICATE broker_refs ({len(set(dupes))} unique keys appear more than once):")
    for k in sorted(set(dupes)):
        print(f"  {k}  (×{all_trades.count(k)})")
else:
    print("No duplicate broker_refs — dedup is not the problem.")
