import { getCurrentWindow } from "@tauri-apps/api/window";
import logo from "../../assets/icon-small.svg";
import { useEffect, useState } from "react";

import "./styles.css";

const appWindow = getCurrentWindow();

export default function App() {
  const [error, setError] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let disposed = false;

    const startUpdate = async () => {
      const progressUnlisten = await appWindow.listen("app://update-progress", (event) => {
        setProgress(event.payload as number);
      });
      const errorUnlisten = await appWindow.listen("app://update-error", () => {
        setError(true);
      });

      if (disposed) {
        progressUnlisten();
        errorUnlisten();
        return;
      }

      unlisteners.push(progressUnlisten, errorUnlisten);
      await appWindow.emit("app://update");
    };

    void startUpdate();

    return () => {
      disposed = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  const handleRetry = () => {
    setError(false);
    setProgress(null);
    appWindow.emit("app://update");
  };

  return (
    <div className="update-window" data-tauri-drag-region>
      <img src={logo} alt="" className="update-logo" data-tauri-drag-region />
      <div className="update-content" data-tauri-drag-region>
        {error ? (
          <DownloadFailed onRetry={handleRetry} />
        ) : (
          <DownloadIndicator progress={progress} />
        )}
      </div>
    </div>
  );
}

function DownloadIndicator({ progress }: { progress: number | null }) {
  return (
    <div className="update-status" data-tauri-drag-region>
      <h1 className="update-title" data-tauri-drag-region>
        Downloading GPGUI
      </h1>
      <p className="update-description" data-tauri-drag-region>
        This may take a moment.
      </p>
      <ProgressWithLabel value={progress} />
    </div>
  );
}

function DownloadFailed({ onRetry }: { onRetry: () => void }) {
  return (
    <div className="update-status error-status" data-tauri-drag-region>
      <div className="error-copy" role="alert" data-tauri-drag-region>
        <h1 className="update-title" data-tauri-drag-region>
          GPGUI update failed
        </h1>
        <p className="update-description" data-tauri-drag-region>
          Please try again.
        </p>
      </div>
      <button type="button" onClick={onRetry} className="retry-button">
        Retry
      </button>
    </div>
  );
}

function ProgressWithLabel({ value }: { value: number | null }) {
  const isDeterminate = value !== null;

  return (
    <div className="progress-row">
      <div
        className={`progress-bar${isDeterminate ? "" : " progress-bar-indeterminate"}`}
        role="progressbar"
        aria-label="Update progress"
        aria-valuemin={isDeterminate ? 0 : undefined}
        aria-valuemax={isDeterminate ? 100 : undefined}
        aria-valuenow={isDeterminate ? Math.round(value) : undefined}
      >
        <div className="progress-fill" style={isDeterminate ? { width: `${value}%` } : undefined} />
      </div>
      <span
        className={`progress-label${value === null ? " progress-label-hidden" : ""}`}
        aria-hidden={value === null}
      >
        {value === null ? "100%" : `${Math.round(value)}%`}
      </span>
    </div>
  );
}
