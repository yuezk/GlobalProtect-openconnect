import { Box, Button, CssBaseline, LinearProgress, Typography } from "@mui/material";
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
    <>
      <CssBaseline />
      <Box className="update-window" data-tauri-drag-region>
        <Box
          component="img"
          src={logo}
          alt="GPGUI"
          className="update-logo"
          data-tauri-drag-region
        />
        <Box className="update-content" data-tauri-drag-region>
          {error ? <DownloadFailed onRetry={handleRetry} /> : <DownloadIndicator progress={progress} />}
        </Box>
      </Box>
    </>
  );
}

function DownloadIndicator({ progress }: { progress: number | null }) {
  return (
    <Box className="update-status" data-tauri-drag-region>
      <Typography component="h1" className="update-title" data-tauri-drag-region>
        Downloading GPGUI
      </Typography>
      <Typography className="update-description" data-tauri-drag-region>
        This may take a moment.
      </Typography>
      <LinearProgressWithLabel value={progress} />
    </Box>
  );
}

function DownloadFailed({ onRetry }: { onRetry: () => void }) {
  return (
    <Box className="update-status error-status" data-tauri-drag-region>
      <Typography component="h1" className="update-title" data-tauri-drag-region>
        GPGUI couldn’t be downloaded
      </Typography>
      <Box className="error-actions" data-tauri-drag-region>
        <Typography className="update-description" data-tauri-drag-region>
          Check your connection and try again.
        </Typography>
        <Button
          variant="contained"
          size="small"
          onClick={onRetry}
          className="retry-button"
        >
          Retry
        </Button>
      </Box>
    </Box>
  );
}

function LinearProgressWithLabel(props: { value: number | null }) {
  const { value } = props;

  return (
    <Box className="progress-row">
      <Box className="progress-track">
        <LinearProgress
          variant={value === null ? "indeterminate" : "determinate"}
          value={value ?? 0}
          className="progress-bar"
          aria-label="Update progress"
        />
      </Box>
      <Typography
        className={`progress-label${value === null ? " progress-label-hidden" : ""}`}
        aria-hidden={value === null}
      >
        {value === null ? "100%" : `${Math.round(value)}%`}
      </Typography>
    </Box>
  );
}
