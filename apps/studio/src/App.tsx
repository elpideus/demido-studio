import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

export function App() {
  useEffect(() => {
    void invoke('window_ready');
  }, []);
  return <div style={{ padding: 24 }}>Demido Studio</div>;
}
