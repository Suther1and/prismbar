import React from 'react';
import ReactDOM from 'react-dom/client';
import './styles/tokens.css';
import { TaskbarView } from './components/TaskbarView';
import { StartView } from './components/StartView';
import { FlyoutView } from './components/FlyoutView';

const view = new URLSearchParams(window.location.search).get('view') || 'taskbar';

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
