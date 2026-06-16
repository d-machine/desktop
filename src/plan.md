# Feature Implementation Plan

Hello Claude, here is a high-level action plan to implement the requested features. Each section outlines the requirement and the files that will likely need modification.

---

## 1. Auto-submit 6-Digit PIN on Entry

**Requirement:**
Automatically submit the login form as soon as the user enters the sixth digit of their PIN.

**Potential Files to Modify:**
- `c:\Users\SUMIT\Workspace\AISPL\desktop\src\components\auth\LoginScreen.tsx`

---

## 2. Empty State for New Users

**Requirement:**
After the initial onboarding, or on subsequent logins where no transactions exist, the user should be directed to a dedicated empty state on the transactions page. This empty state should be a separate, reusable component.

**Potential Files to Modify:**
- `c:\Users\SUMIT\Workspace\AISPL\desktop\src\components\layout\AppLayout.tsx`
- `c:\Users\SUMIT\Workspace\AISPL\desktop\src\pages\TransactionsPage.tsx`
- New File: `c:\Users\SUMIT\Workspace\AISPL\desktop\src\components\transactions\EmptyState.tsx`

---

## 3. Consistent Casing for "Portfolio" and "Accounts"

**Requirement:**
Ensure "Portfolio" and "Accounts" labels are consistently styled in ALL CAPS wherever they appear.

**Potential Files to Modify:**
- A project-wide search for "Portfolio" and "Account" will be required to identify all relevant component files. Examples include filter buttons, table headers, and sidebar links.

---

## 4. Widen the Import Popup

**Requirement:**
Set the import popup's width to 90% of the page width to prevent layout shifts caused by long file names.

**Potential Files to Modify:**
- The component that defines the import dialog, likely `c:\Users\SUMIT\Workspace\AISPL\desktop\src\components\transactions\ImportDialog.tsx`.
