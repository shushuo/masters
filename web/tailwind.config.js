/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        // Panda brand — deep-slate canvas, warm cream text, sage-teal accent
        // (mirrors the desktop dark palette in ui/desktop/src/index.css).
        ink: {
          950: '#151b1e',
          900: '#1a2125',
          800: '#212a2f',
          700: '#2a343a',
          600: '#313c42',
        },
        cream: {
          DEFAULT: '#eceadd',
          soft: '#d8d6c8',
        },
        mist: {
          DEFAULT: '#9aa6a8',
          faint: '#6c777c',
        },
        accent: {
          DEFAULT: '#8fb3ad',
          soft: '#a6c6c1',
          glow: '#7ea39c',
        },
      },
      fontFamily: {
        sans: ['Inter', 'ui-sans-serif', 'system-ui', 'sans-serif'],
      },
    },
  },
  plugins: [],
}
