/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    "./frontend/src/**/*.{html,js}",
  ],
  theme: {
    extend: {
      colors: {
        // Custom colors matching the original design
        primary: {
          50: '#f0fdf4',
          500: '#22c55e', // green-500 (close to #27ae60)
          600: '#16a34a', // green-600 (close to #219a52)
        },
        secondary: {
          500: '#ef4444', // red-500 (close to #e74c3c)
          600: '#dc2626', // red-600 (close to #c0392b)
        },
        info: {
          500: '#3b82f6', // blue-500 (close to #3498db)
          600: '#2563eb', // blue-600 (close to #2980b9)
        },
        neutral: {
          400: '#94a3b8', // slate-400 (close to #95a5a6)
          500: '#64748b', // slate-500 (close to #7f8c8d)
          700: '#334155', // slate-700 (close to #2c3e50)
          800: '#1e293b', // slate-800 (close to #34495e)
        },
        warning: '#f59e0b', // amber-500 (close to #f39c12)
        error: '#f97316',   // orange-500 (close to #e67e22)
      },
      animation: {
        'pulse-dot': 'pulse-dot 1s infinite',
      },
      keyframes: {
        'pulse-dot': {
          '0%, 100%': { opacity: '1' },
          '50%': { opacity: '0.5' }
        }
      },
      fontFamily: {
        'mono': ['Monaco', 'Menlo', 'Ubuntu Mono', 'monospace'],
      }
    },
  },
  plugins: [],
}