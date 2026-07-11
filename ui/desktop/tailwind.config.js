/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "media",
  theme: {
    extend: {
      colors: {
        bg: "var(--color-bg)",
        surface: "var(--color-surface)",
        "surface-2": "var(--color-surface-2)",
        border: {
          DEFAULT: "var(--color-border)",
          strong: "var(--color-border-strong)",
        },
        // Flat keys so utilities read as `text-muted`/`text-faint` (nesting under
        // `text` would generate `text-text-muted`). `text` stays a string → `text-text`.
        text: "var(--color-text)",
        muted: "var(--color-text-muted)",
        faint: "var(--color-text-faint)",
        accent: {
          DEFAULT: "var(--color-accent)",
          hover: "var(--color-accent-hover)",
          fg: "var(--color-accent-fg)",
          subtle: "var(--color-accent-subtle)",
        },
        brand: {
          DEFAULT: "var(--color-brand)",
          sage: "var(--color-brand-sage)",
        },
        tool: {
          bg: "var(--color-tool-bg)",
          fg: "var(--color-tool-fg)",
          border: "var(--color-tool-border)",
        },
        success: {
          DEFAULT: "var(--color-success)",
          hover: "var(--color-success-hover)",
        },
        danger: {
          DEFAULT: "var(--color-danger)",
          hover: "var(--color-danger-hover)",
          bg: "var(--color-danger-bg)",
          fg: "var(--color-danger-fg)",
        },
        warning: {
          DEFAULT: "var(--color-warning)",
          bg: "var(--color-warning-bg)",
          fg: "var(--color-warning-fg)",
        },
        info: {
          DEFAULT: "var(--color-info)",
          bg: "var(--color-info-bg)",
          fg: "var(--color-info-fg)",
        },
      },
      borderRadius: {
        sm: "var(--radius-sm)",
        DEFAULT: "var(--radius)",
        lg: "var(--radius-lg)",
      },
      boxShadow: {
        sm: "var(--shadow-sm)",
        DEFAULT: "var(--shadow)",
      },
      fontFamily: {
        sans: [
          "Inter var",
          "Inter",
          "ui-sans-serif",
          "system-ui",
          "-apple-system",
          "Segoe UI",
          "sans-serif",
        ],
        // The wordmark / screen-title face. Maps to `font-display` (used in MastersHub);
        // currently the same Inter stack — retune via `--font-display` if a display cut lands.
        display: ["var(--font-display)"],
      },
    },
  },
  plugins: [],
};
