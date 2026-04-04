/** @type {import('tailwindcss').Config} */
export default {
  content: ['./src/**/*.{astro,html,js,ts}'],
  theme: {
    extend: {
      colors: {
        base:    '#0b0b14',
        surface: '#13131f',
        card:    '#1c1c2e',
        border:  '#2a2a42',
        accent: {
          DEFAULT: '#7c6af7',
          light:   '#a78bfa',
          dark:    '#5a4fcf',
        },
      },
      fontFamily: {
        sans: ['"Geist"', '"Segoe UI"', 'system-ui', 'sans-serif'],
        mono: ['"Geist Mono"', '"Cascadia Code"', 'Consolas', 'monospace'],
      },
    },
  },
  plugins: [],
};
