/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    screens: {
      'sm': '640px',
      'md': '768px',
      'lg': '1024px',
      'xl': '1280px',
      '2xl': '1536px',
    },
    extend: {
      colors: {
        // Custom color palette inspired by Day One and Bear
        journal: {
          50: '#fafaf9',
          100: '#f5f5f4',
          200: '#e7e5e4',
          300: '#d6d3d1',
          400: '#a8a29e',
          500: '#78716c',
          600: '#57534e',
          700: '#44403c',
          800: '#292524',
          900: '#1c1917',
        },
        accent: {
          50: '#eff6ff',
          100: '#dbeafe',
          200: '#bfdbfe',
          300: '#93c5fd',
          400: '#60a5fa',
          500: '#3b82f6',
          600: '#2563eb',
          700: '#1d4ed8',
          800: '#1e40af',
          900: '#1e3a8a',
        },
        surface: {
          primary: '#ffffff',
          secondary: '#fafaf9',
          tertiary: '#f5f5f4',
        },
        text: {
          primary: '#1c1917',
          secondary: '#44403c',
          tertiary: '#78716c',
          muted: '#a8a29e',
        },
        // Default border color token to support `border-border`
        border: '#e7e5e4',
      },
      fontFamily: {
        sans: [
          'Inter',
          'ui-sans-serif',
          'system-ui',
          '-apple-system',
          'BlinkMacSystemFont',
          'Segoe UI',
          'Roboto',
          'Helvetica Neue',
          'Arial',
          'sans-serif',
        ],
        mono: [
          'SF Mono',
          'Monaco',
          'Cascadia Code',
          'Roboto Mono',
          'Consolas',
          'monospace',
        ],
      },
      fontSize: {
        'journal-title': ['clamp(1.75rem, 4vw, 2.25rem)', { lineHeight: '1.2', fontWeight: '700' }],
        'journal-heading': ['clamp(1.5rem, 3.5vw, 1.875rem)', { lineHeight: '1.3', fontWeight: '600' }],
        'journal-subheading': ['clamp(1.25rem, 3vw, 1.5rem)', { lineHeight: '1.4', fontWeight: '600' }],
        'journal-body': ['clamp(0.875rem, 2vw, 1rem)', { lineHeight: '1.6', fontWeight: '400' }],
        'journal-small': ['clamp(0.75rem, 1.5vw, 0.875rem)', { lineHeight: '1.25rem', fontWeight: '400' }],
      },
      spacing: {
        'journal': '1.5rem',
        'journal-sm': '1rem',
        'journal-lg': '2rem',
      },
      boxShadow: {
        'journal': '0 2px 8px rgba(0, 0, 0, 0.04)',
        'journal-lg': '0 4px 16px rgba(0, 0, 0, 0.08)',
        'journal-focus': '0 0 0 3px rgba(59, 130, 246, 0.1)',
      },
      borderRadius: {
        'journal': '0.5rem',
        'journal-sm': '0.375rem',
      },
      animation: {
        'fade-in': 'fadeIn 0.15s ease-in-out',
        'slide-in': 'slideIn 0.2s ease-out',
      },
      keyframes: {
        fadeIn: {
          '0%': { opacity: '0' },
          '100%': { opacity: '1' },
        },
        slideIn: {
          '0%': { transform: 'translateY(-4px)', opacity: '0' },
          '100%': { transform: 'translateY(0)', opacity: '1' },
        },
      },
    },
  },
  plugins: [
    require('@tailwindcss/typography'),
    require('tailwindcss-animate'),
  ],
}