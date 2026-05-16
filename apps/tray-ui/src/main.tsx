import React from 'react';
import ReactDOM from 'react-dom/client';
// Geist + Geist Mono are the design language's visual signature.
// Bundled via @fontsource-variable so they ship in the Tauri webview's
// asset bundle — fetching from fonts.googleapis.com is blocked by the
// app's CSP (`default-src 'self'`).
import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import App from './App';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
