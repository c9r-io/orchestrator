# Portal Design System (Platform Template)

This document defines the design system conventions for the platform's default frontend (`portal/`). It is a reusable template and is not bound to any specific business copy or entities (avoid domain-specific nouns like tenant/user in the design system itself).

Goals:
- Provide a consistent visual language and component standards to reduce UI rework.
- Ensure accessibility and performance (especially fallbacks for blur/transparency).
- Give AI-generated UI clear constraints (tokens, component variants, interactions, layout rules).

## 1. Design Language: Liquid Glass (Optional)

The default recommended look is "Liquid Glass": translucent surfaces + backdrop blur + soft shadows + large radii.

Principles:
- Readability first: transparency must not reduce text contrast
- Depth layering: use blur + shadow to express hierarchy
- Consistency: similar containers share the same glass parameters
- Performance: provide fallbacks for environments that do not support `backdrop-filter`

## 2. Design Tokens

Use CSS variables to manage tokens (trim as needed per project).

### Light Mode (`:root`)

```css
:root {
  --bg-primary: #f2f2f7;
  --bg-secondary: #ffffff;
  --bg-tertiary: #e5e5ea;

  --glass-bg: rgba(255, 255, 255, 0.72);
  --glass-bg-hover: rgba(255, 255, 255, 0.85);
  --glass-border: rgba(255, 255, 255, 0.5);
  --glass-border-subtle: rgba(0, 0, 0, 0.06);
  --glass-shadow: rgba(0, 0, 0, 0.08);
  --glass-shadow-strong: rgba(0, 0, 0, 0.15);
  --glass-highlight: rgba(255, 255, 255, 0.9);
  --glass-illumination: rgba(255, 255, 255, 0.4);

  --text-primary: #1d1d1f;
  --text-secondary: #86868b;
  --text-tertiary: #aeaeb2;
  --text-inverse: #ffffff;

  --accent: #007aff;
  --accent-tint: rgba(0, 122, 255, 0.12);
  --danger: #ff3b30;
  --danger-tint: rgba(255, 59, 48, 0.12);
}
```

### Dark Mode (`[data-theme="dark"]`)

```css
[data-theme="dark"] {
  --bg-primary: #000000;
  --bg-secondary: #1c1c1e;
  --bg-tertiary: #2c2c2e;

  --glass-bg: rgba(44, 44, 46, 0.65);
  --glass-bg-hover: rgba(58, 58, 60, 0.75);
  --glass-border: rgba(255, 255, 255, 0.1);
  --glass-border-subtle: rgba(255, 255, 255, 0.05);
  --glass-shadow: rgba(0, 0, 0, 0.4);
  --glass-shadow-strong: rgba(0, 0, 0, 0.6);
  --glass-highlight: rgba(255, 255, 255, 0.15);
  --glass-illumination: rgba(255, 255, 255, 0.05);

  --text-primary: #ffffff;
  --text-secondary: #98989d;
  --text-tertiary: #636366;
  --text-inverse: #000000;

  --accent-tint: rgba(0, 122, 255, 0.2);
  --danger-tint: rgba(255, 59, 48, 0.2);
}
```

## 3. Base Component Styles

### Glass Containers (Card/Panel)

```css
.liquid-glass {
  position: relative;
  background: var(--glass-bg);
  backdrop-filter: blur(24px) saturate(180%);
  -webkit-backdrop-filter: blur(24px) saturate(180%);
  border: 1px solid var(--glass-border);
  border-radius: 20px;
  box-shadow:
    0 8px 32px var(--glass-shadow),
    inset 0 1px 0 var(--glass-highlight),
    inset 0 -1px 0 rgba(0, 0, 0, 0.05);
}

.liquid-glass::before {
  content: "";
  position: absolute;
  inset: 0;
  border-radius: inherit;
  background: linear-gradient(135deg, var(--glass-illumination) 0%, transparent 50%);
  pointer-events: none;
}

.liquid-glass:hover {
  background: var(--glass-bg-hover);
  box-shadow:
    0 12px 40px var(--glass-shadow-strong),
    inset 0 1px 0 var(--glass-highlight),
    inset 0 -1px 0 rgba(0, 0, 0, 0.05);
  transform: translateY(-2px);
}
```

Fallback:

```css
@supports not (backdrop-filter: blur(24px)) {
  .liquid-glass {
    background: var(--bg-secondary);
  }
}
```

## 4. Layout Conventions

- Page container: max width + centered + sensible whitespace
- Grid gap: 16px by default
- Card padding: 20px
- Radius: cards 20px, buttons/inputs 12px

## 5. Interaction And Accessibility

- All interactive elements must be keyboard-accessible
- Focus rings must be visible (do not rely on subtle color differences)
- Text/background contrast must meet requirements (especially on translucent glass)
- Animation duration: 200-300ms recommended; avoid overly long easing

## 6. Theme Persistence (Recommended)

Theme selection can be persisted to localStorage. Use a project-agnostic key:
- `theme`

## 7. Component Examples (Generic)

Suggested button variants:
- primary (accent background)
- secondary (glass background)
- outline (border)
- ghost (no background)
- destructive (danger background)

Examples (pseudo-code):

```tsx
<Button>Primary Action</Button>
<Button variant="secondary">Secondary</Button>
<Button variant="outline">Outline</Button>
<Button variant="ghost">Ghost</Button>
<Button variant="destructive">Delete</Button>
```
