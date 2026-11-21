import React from 'react';
import ReactDOM from 'react-dom/client';
import './styles/tokens.css';
import { TaskbarView } from './components/TaskbarView';
import { StartView } from './components/StartView';
import { FlyoutView } from './components/FlyoutView';
import {
  getBackdropMode,
  ipc,
  isTauri,
  onEvent,
  type BackdropMode,
  type GlassDepth,
  type TestBarConfig,
} from './lib/ipc';

const view = new URLSearchParams(window.location.search).get('view') || 'taskbar';

function applyBackdropMode(mode: unknown) {
  if (mode === 'dcomp' || mode === 'swca' || mode === 'none') {
    document.documentElement.dataset.backdrop = mode;
  }
}

function applyGlassDepth(depth: unknown) {
  if (depth === 'crystal' || depth === 'obsidian' || depth === 'matte') {
    document.documentElement.dataset.glass = depth;
  } else {
    document.documentElement.dataset.glass = 'obsidian';
  }
}

if (typeof document !== 'undefined') {
  document.documentElement.dataset.backdrop = 'dcomp';
  document.documentElement.dataset.glass = 'obsidian';

  if (isTauri()) {
    void getBackdropMode()
      .then((mode) => applyBackdropMode(mode))
      .catch(() => {
        applyBackdropMode('none');
      });
    void onEvent<BackdropMode | { mode?: BackdropMode }>('backdrop-mode-updated', (payload) => {
      const mode = typeof payload === 'string' ? payload : payload?.mode;
      applyBackdropMode(mode);
    });
    void ipc<TestBarConfig>('get_config')
      .then((cfg) => applyGlassDepth(cfg?.glassDepth))
      .catch(() => {});
    void onEvent<TestBarConfig>('config-updated', (cfg) => {
      applyGlassDepth(cfg?.glassDepth as GlassDepth | undefined);
    });
  }
}

const Root: React.FC = () => {
  if (view === 'start') return <StartView />;
  if (view === 'flyout') return <FlyoutView />;
  return <TaskbarView />;
};

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
