# Design System Specification: The Luminous Void

## 1. Overview & Creative North Star
This design system is built upon the Creative North Star of **"The Luminous Void."** In a market saturated with "flat" SaaS templates, this system seeks to reclaim depth, intentionality, and editorial soul. We are moving away from the rigid, boxed-in constraints of traditional dashboards and toward an experience that feels like a high-end physical object—layered, tactile, and illuminated from within.

We achieve this through **Intentional Asymmetry** and **Tonal Depth**. By leveraging extreme typographic scales and breaking the standard grid with overlapping containers, we create a "Digital Curator" aesthetic. The layout doesn't just display data; it presents it with authority.

## 2. Colors & Surface Philosophy
The palette is rooted in deep, obsidian charcoals, punctuated by high-energy electric gradients that represent "light" in the darkness.

### The "No-Line" Rule
To maintain a premium editorial feel, **1px solid borders are prohibited for sectioning.** Structural boundaries must be defined through:
1.  **Background Color Shifts:** Use `surface_container_low` containers sitting on a `surface` background.
2.  **Tonal Transitions:** Use subtle shifts between `surface_container_lowest` and `surface_container_highest` to imply edge and hierarchy.

### Surface Hierarchy & Nesting
Think of the UI as stacked sheets of smoked glass. 
*   **Base Layer:** `surface` (#0e0e0e)
*   **Sectioning:** `surface_container_low` (#131313)
*   **Interactive Cards:** `surface_container` (#1a1a1a)
*   **Floating/Elevated Elements:** `surface_container_high` (#20201f) or `highest` (#262626).

### The "Glass & Gradient" Rule
For elements that need to feel "alive," use **Glassmorphism**. Apply a semi-transparent `surface_variant` with a heavy `backdrop-blur`. 
**Signature Textures:** Main CTAs must use a linear gradient from `primary` (#94aaff) to `secondary` (#a68cff). This provides the "visual soul" that differentiates this design system from generic dark themes.

## 3. Typography: Editorial Authority
We use **Inter** as our typographic backbone. The goal is high-contrast hierarchy.

*   **The Hero Moment:** Use `display-lg` (3.5rem) for primary data points or welcome headers. The sheer scale creates a sense of luxury and confidence.
*   **The Narrative:** Use `body-lg` (1rem) for content. Ensure line-height is generous (1.6) to allow the "Void" to breathe.
*   **The Utility:** `label-sm` (0.6875rem) should be used for metadata, always set in `on_surface_variant` (#adaaaa) to keep it secondary to the data.

## 4. Elevation & Depth
Elevation is achieved through **Tonal Layering** rather than structural lines.

### The Layering Principle
Depth is organic. Place a `surface_container_lowest` card on a `surface_container_low` section to create a soft "recessed" or "lifted" look. Avoid heavy-handed shadows.

### Ambient Shadows
When an element must float (e.g., a dropdown or modal):
*   **Blur:** 24px - 40px.
*   **Opacity:** 4% - 8%.
*   **Color:** Use a tinted version of `on_surface` rather than pure black. This mimics natural light bouncing off dark materials.

### The "Ghost Border" Fallback
If accessibility demands a border, use a **Ghost Border**: the `outline_variant` token at 15% opacity. Never use 100% opaque borders; they disrupt the "Void" aesthetic.

## 5. Components

### Buttons
*   **Primary:** Gradient (`primary` to `secondary`). Roundedness: `md` (0.75rem). No border.
*   **Secondary:** Glassmorphic. `surface_variant` at 40% opacity + backdrop blur.
*   **Tertiary:** Transparent background with `primary` text. Use `title-sm` for the label.

### Cards & Lists
*   **Card Anatomy:** Use `surface_container` (#1a1a1a) with a `DEFAULT` (0.5rem) or `md` (0.75rem) corner radius.
*   **Forbid Dividers:** Do not use line separators between list items. Use vertical whitespace (Spacing Scale `4` or `5`) or a 1-step background shift (`surface_container_low` to `surface_container`) to define rows.

### Input Fields
*   **Base State:** `surface_container_highest` (#262626).
*   **Focus State:** A 1px "Ghost Border" using `primary_dim` and a subtle outer glow (4px blur) using the `primary` color at 20% opacity.

### Floating Action Tabs (Specialty Component)
Instead of a standard sidebar, use a floating navigation dock styled with `surface_container_high`, `backdrop-blur`, and a `full` (9999px) roundedness scale.

## 6. Do's and Don'ts

### Do
*   **Do** embrace negative space. If a layout feels "full," increase spacing by one level on the scale (e.g., move from `8` to `10`).
*   **Do** use `secondary` (#a68cff) for subtle highlights in data visualization to complement the `primary` blue.
*   **Do** apply `smooth micro-interactions`: 200ms Easing (Cubic-Bezier) for all hover states.

### Don't
*   **Don't** use 100% white (#ffffff) for long-form body text; use `on_surface_variant` to prevent eye strain.
*   **Don't** use "Drop Shadows" that have a hard offset. Keep `x` and `y` offsets near 0 to maintain the "Ambient Light" look.
*   **Don't** use sharp corners. Everything must adhere to the `8-12px` (DEFAULT to md) range to feel approachable and modern.