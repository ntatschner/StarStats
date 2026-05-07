import { useCallback, useEffect, useState } from 'react';
import { api, type Config } from './api';
import { StatusPane } from './components/StatusPane';
import { SettingsPane } from './components/SettingsPane';
import { LogsPane } from './components/LogsPane';
import { TrayHeader, type TrayView } from './components/TrayHeader';
import { useStatusPolling } from './hooks/useStatusPolling';
import './styles.css';

export default function App() {
  const [view, setView] = useState<TrayView>('status');
  const [config, setConfig] = useState<Config | null>(null);
  const [error, setError] = useState<string | null>(null);

  const { status, refresh: refreshStatus } = useStatusPolling({
    active: view === 'status',
    onError: setError,
  });

  const refreshConfig = useCallback(async () => {
    try {
      const c = await api.getConfig();
      setConfig(c);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refreshConfig();
  }, [refreshConfig]);

  const onSaveConfig = async (next: Config) => {
    await api.saveConfig(next);
    setConfig(next);
    void refreshStatus();
  };

  const isTailing = status?.tail.current_path != null;

  return (
    <div className="app">
      <TrayHeader
        view={view}
        onView={setView}
        isTailing={isTailing}
        version={__APP_VERSION__}
      />
      <main className="app__main">
        {error && <div className="error">Error: {error}</div>}
        {view === 'status' &&
          (status ? (
            <StatusPane
              status={status}
              webOrigin={
                config?.web_origin ?? config?.remote_sync.api_url ?? null
              }
              onGoToSettings={() => setView('settings')}
            />
          ) : (
            <div className="loading">Loading…</div>
          ))}
        {view === 'logs' && <LogsPane />}
        {view === 'settings' &&
          (config ? (
            <SettingsPane config={config} onSave={onSaveConfig} />
          ) : (
            <div className="loading">Loading…</div>
          ))}
      </main>
    </div>
  );
}
