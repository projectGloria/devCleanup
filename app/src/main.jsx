import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App.jsx';
import './styles.css';

if (import.meta.env.PROD) {
  // Native-app feel: no browser "Back / Reload / Inspect" menu, except in text fields
  // where cut/copy/paste is expected.
  window.addEventListener('contextmenu', (e) => {
    if (!e.target.closest('input, textarea')) e.preventDefault();
  });
  // A reload would drop the Channel of a running delete (the Rust side keeps going,
  // but the UI would lose track of it), and print/find make no sense in this app.
  window.addEventListener('keydown', (e) => {
    const k = e.key.toLowerCase();
    if (k === 'f5' || ((e.ctrlKey || e.metaKey) && ['r', 'p', 'f', 'g', 'u'].includes(k))) e.preventDefault();
  });
}

createRoot(document.getElementById('root')).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
