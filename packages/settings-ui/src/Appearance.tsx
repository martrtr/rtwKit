import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { connectPanel, type Client, type Theme } from '@rintawa/bridge';
import { Status } from '@rintawa/ui';
import '@rintawa/ui/style.css';
import './style.css';
function App() {
  const [client, setClient] = useState<Client | null>(null), [theme, setTheme] = useState<Theme>({mode:'dark'}), [error, setError] = useState('');
  useEffect(() => { let disposed = false, bridge: Client | undefined; connectPanel(value => { if (!disposed) setTheme(value); }).then(value => { if (disposed) value.dispose(); else { bridge = value; setClient(value); } }).catch(error => setError(error.message)); return () => { disposed = true; bridge?.dispose(); }; }, []);
  return <main className="settings-general"><h3>Appearance</h3><label htmlFor="settings-theme">Theme</label><select id="settings-theme" disabled={!client} value={theme.mode} onChange={event => void client?.request('ui.theme.set',{theme:{mode:event.target.value}}).catch(error => setError(error.message))}><option value="dark">AMOLED black</option><option value="light">Light</option></select>{error && <Status error>{error}</Status>}</main>;
}
createRoot(document.getElementById('root')!).render(<App />);
