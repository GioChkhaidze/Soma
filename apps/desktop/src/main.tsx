import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './app/App';
import './shared/styles/styles.css';

const root = document.getElementById('root');

if (!root) {
  throw new Error('Soma root element was not found.');
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>
);
