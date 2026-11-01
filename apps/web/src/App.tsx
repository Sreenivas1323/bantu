import { useState } from "react";
import { ConnectScreen } from "./components/ConnectScreen.tsx";
import { Shell } from "./components/Shell.tsx";
import { clearConnection, loadConnection, saveConnection, type Connection } from "./lib/connection.ts";

export function App() {
  const [conn, setConn] = useState<Connection | null>(() => loadConnection());
  const [authError, setAuthError] = useState<string | null>(null);

  if (!conn) {
    return (
      <ConnectScreen
        error={authError}
        onConnect={(next) => {
          saveConnection(next);
          setAuthError(null);
          setConn(next);
        }}
      />
    );
  }

  return (
    <Shell
      key={`${conn.baseUrl}|${conn.token}`}
      conn={conn}
      onDisconnect={(reason) => {
        clearConnection();
        setAuthError(reason ?? null);
        setConn(null);
      }}
    />
  );
}
