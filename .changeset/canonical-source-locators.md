---
"@sanurb/shiro-cli": minor
---

feat: preserve parser-neutral page and region source locators as canonical block provenance

Docling page numbers, bounding boxes, and page dimensions now survive translation and atomic graph persistence. Search, context, block reads, and immutable explain snapshots expose validated locators without leaking Docling schema types. Existing documents remain valid with empty locator lists.
