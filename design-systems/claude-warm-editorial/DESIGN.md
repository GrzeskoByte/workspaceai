# Claude Warm Editorial

> Default design system for Harness artifacts. Source: DESIGN-claude.md (Claude-design-analysis).

## Principles
- Tinted cream canvas, never pure white. Depth comes from cream-vs-dark surfaces, not shadows.
- Coral is scarce: one primary action per view. Small coral text uses primary-active for contrast.
- Serif display headings over a clean sans body; monospace for code.
- Generous spacing; cards with 12px radius; 1px hairlines.

## Colour tokens (light / dark)
| Token | Light | Dark | Use |
|---|---|---|---|
| primary | #cc785c | #cc785c | Coral |
| primary-active | #a9583e | #a9583e | Pressed coral |
| primary-disabled | #e6dfd8 | #2e2b27 | Disabled CTA fill, with muted text |
| canvas | #faf9f5 | #181715 | Page floor |
| surface-soft | #f5f0e8 | #1f1e1b | Section dividers, very soft bands, sidebars |
| surface-card | #efe9de | #252320 | Feature and content cards, active category tab |
| surface-cream-strong | #e8e0d2 | #2e2b27 | Selected tabs, emphasised bands |
| surface-dark | #181715 | #181715 | Code windows, product mockups, featured pricing tier, footer |
| surface-dark-elevated | #252320 | #252320 | Elevated panels and secondary buttons inside dark cards |
| surface-dark-soft | #1f1e1b | #1f1e1b | Inner code block inside a larger dark card |
| ink | #141413 | #faf9f5 | Headlines and primary text on canvas (18 |
| body-strong | #252523 | #faf9f5 | Lead paragraphs, emphasised body |
| body | #3d3d3a | #d6d3cc | Default running text (10 |
| muted | #6c6a64 | #a09d96 | Sub-headings, breadcrumbs, secondary labels (5 |
| muted-soft | #8e8b82 | #8a867e | Captions and fine print |
| hairline | #e6dfd8 | #2e2b27 | 1px borders on cream surfaces |
| hairline-soft | #ebe6df | #252320 | Barely visible divider inside one band |
| on-primary | #ffffff | #ffffff | Text on coral fills (see the contrast note on primary) |
| on-dark | #faf9f5 | #faf9f5 | Cream-tinted white on dark surfaces |
| on-dark-soft | #a09d96 | #a09d96 | Footer body and secondary labels on dark surfaces (6 |
| accent-teal | #5db8a6 | #5db8a6 | Sparingly: terminal status, active-connection dots |
| accent-amber | #e8a55a | #e8a55a | Category badges and small inline highlights |
| success | #5db872 | #5db872 | Green status dots, available indicators |
| warning | #d4a017 | #d4a017 | Warning callouts (rare) |
| error | #c64545 | #c64545 | Validation errors (4 |

## Type
- Display: Copernicus, "Tiempos Headline", "Cormorant Garamond", "EB Garamond", Georgia, serif
- Sans: StyreneB, Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif
- Mono: "JetBrains Mono", ui-monospace, monospace

| Style | Size | Line height | Weight | Use |
|---|---|---|---|---|
| display-xl | 64px | 1.05 | 400 | Hero h1. |
| display-lg | 48px | 1.1 | 400 | Section heads. |
| display-md | 36px | 1.15 | 400 | Sub-section heads, model names. |
| display-sm | 28px | 1.2 | 400 | Pricing tier names, callout headlines, prices. |
| title-lg | 22px | 1.3 | 500 | Plan size labels. |
| title-md | 18px | 1.4 | 500 | Feature card titles, intro paragraphs. |
| title-sm | 16px | 1.4 | 500 | Connector tile titles, list labels. |
| body-md | 16px | 1.55 | 400 | Default running text. |
| body-sm | 14px | 1.55 | 400 | Footer body, fine print, dense UI. |
| caption | 13px | 1.4 | 500 | Badge labels, captions. |
| caption-uppercase | 12px | 1.4 | 500 | Category tags, NEW badges. Set uppercase. |
| button | 14px | 1 | 500 | Button labels. |
| nav-link | 14px | 1.4 | 500 | Top-nav items. |
| code | 14px | 1.6 | 400 | Code blocks and terminal text. |

## Spacing
- space-xxs: 4px — Base unit; icon-to-label gaps.
- space-xs: 8px — Tight stacks, chip gaps.
- space-sm: 12px — Button vertical padding, list gaps.
- space-md: 16px — Default gap between related elements.
- space-lg: 24px — Code-window and connector-tile padding; cookie card.
- space-xl: 32px — Feature, pricing and model card padding.
- space-xxl: 48px — Coral callout padding.
- space-section: 96px — Between major bands; hero vertical padding.

## Radius
- radius-xs: 4px — Badge accents, tiny dropdowns.
- radius-sm: 6px — Small inline buttons, dropdown items.
- radius-md: 8px — Buttons, inputs, category tabs.
- radius-lg: 12px — Content and product cards.
- radius-xl: 16px — Hero illustration container, marquee components.
- radius-pill: 9999px — Badges, NEW tags, avatars, icon buttons.

## CSS variables
```css
:root{
  --primary: #cc785c;
  --primary-active: #a9583e;
  --primary-disabled: #e6dfd8;
  --canvas: #faf9f5;
  --surface-soft: #f5f0e8;
  --surface-card: #efe9de;
  --surface-cream-strong: #e8e0d2;
  --surface-dark: #181715;
  --surface-dark-elevated: #252320;
  --surface-dark-soft: #1f1e1b;
  --ink: #141413;
  --body-strong: #252523;
  --body: #3d3d3a;
  --muted: #6c6a64;
  --muted-soft: #8e8b82;
  --hairline: #e6dfd8;
  --hairline-soft: #ebe6df;
  --on-primary: #ffffff;
  --on-dark: #faf9f5;
  --on-dark-soft: #a09d96;
  --accent-teal: #5db8a6;
  --accent-amber: #e8a55a;
  --success: #5db872;
  --warning: #d4a017;
  --error: #c64545;
}
```
