# UI/UX Test - Lists, Tables, And Data-Dense Pages

**Module**: Common Components And States  
**Scope**: List/table readability, sorting/pagination, bulk actions, empty/loading/error states  
**Scenarios**: 5

---

## Scenario 1: Header And Row Readability (Alignment And Density)

### Goal
Verify data-dense regions are not cramped, columns align predictably, and row height is reasonable.

### Steps
1. Open a table or list page (`{list_route}`).
2. Check:
   - Header font weight/size and tracking (if using uppercase)
   - Inline badges/icons/action buttons are vertically centered

### Expected
- Clear separation between headers and rows (divider line or spacing).
- Hovering a row does not change row height.

---

## Scenario 2: Search/Filter/Sort State Is Understandable And Reversible

### Goal
Verify users can understand current filters and clear them in one action.

### Steps
1. Apply one filter + one search + one sort.
2. Reload the page and use back/forward navigation.
3. Clear conditions.

### Expected
- Current conditions are visible (chips/inputs/URL query).
- Clear action is obvious and does not delete data.

---

## Scenario 3: Pagination/Infinite Scroll Strategy Is Consistent

### Goal
Verify pagination/infinite scroll behavior is consistent and users do not lose their place.

### Steps
1. Go to page 2 / scroll to load more.
2. Navigate away and return.

### Expected
- Pagination: current page is explicit; switching pages does not jump unexpectedly.
- Infinite scroll: returning restores position according to policy or provides "back to top".

---

## Scenario 4: Empty/Loading/Error States Provide Correct Information And Actions

### Goal
Verify users always have a clear next step when there is no data, while loading, or on load failures.

### Steps
1. Empty data: visit a list with no items.
2. Loading: simulate slow network (Network throttling).
3. Error: simulate 500 or offline.

### Expected
- Empty: explains why + primary action (create/import/refresh) + secondary action (docs/help).
- Loading: shows skeleton/spinner and does not cause layout jitter.
- Error: provides retry and does not lose user input/conditions (per project policy).

---

## Scenario 5: Bulk Actions And Confirmation For Dangerous Actions

### Goal
Verify bulk delete/disable/export actions have consistent confirmation and feedback and reduce accidental clicks.

### Steps
1. Select multiple rows (checkbox).
2. Trigger a bulk action.
3. Cancel once and confirm once.

### Expected
- Destructive actions require a second confirmation (per project policy).
- Success/failure feedback is clear (toast/inline) and the affected scope is traceable.

