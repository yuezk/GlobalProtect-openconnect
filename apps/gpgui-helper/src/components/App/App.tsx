import { Box, Button, CssBaseline, LinearProgress, Typography } from "@mui/material";
import { appWindow } from "@tauri-apps/api/window";
import logo from "../../assets/icon.svg";
import { useEffect, useState } from "react";

import "./styles.css";

function useUpdateProgress() {
  const [progress, setProgress] = useState<number | null>(null);

  useEffect(() => {
    const unlisten = appWindow.listen("app://update-progress", (event) => {
      setProgress(event.payload as number);
    });

    return () => {
      unlisten.then((unlisten) => unlisten());
    };
  }, []);

  return progress;
}

export default function App() {
  const [error, setError] = useState(false);

  useEffect(() => {
    const unlisten = appWindow.listen("app://update-error", () => {
      setError(true);
    });

    return () => {
      unlisten.then((unlisten) => unlisten());
    };
  }, []);

  const handleRetry = () => {
    setError(false);
    appWindow.emit("app://update");
  };

  return (
    <>
      <CssBaseline />
      <Box
        sx={{ position: "absolute", inset: 0 }}
        display="flex"
        alignItems="center"
        px={2}
        data-tauri-drag-region
      >
        <Box display="flex" alignItems="center" flex="1" data-tauri-drag-region>
          <Box
            component="img"
            src={logo}
            alt="logo"
            sx={{ width: "4rem", height: "4rem" }}
            data-tauri-drag-region
          />
          <Box flex={1} ml={2}>
            {error ? <DownloadFailed onRetry={handleRetry} /> : <DownloadIndicator />}
          </Box>
        </Box>
      </Box>
    </>
  );
}

function DownloadIndicator() {
  const progress = useUpdateProgress();

  return (
    <>
      <Typography variant="h1" fontSize="1rem" data-tauri-drag-region>
        Updating the GUI components...
      </Typography>
      <Box mt={1}>
        <LinearProgressWithLabel value={progress} />
      </Box>
    </>
  );
}

function DownloadFailed({ onRetry }: { onRetry: () => void }) {
  return (
    <>
      <Typography variant="h1" fontSize="1rem" data-tauri-drag-region>
        Failed to update the GUI components.
      </Typography>
      <Box mt={1} data-tauri-drag-region>
        <Button
          variant="contained"
          color="primary"
          size="small"
          onClick={onRetry}
          sx={{
            textTransform: "none",
          }}
        >
          Retry
        </Button>
      </Box>
    </>
  );
}

function LinearProgressWithLabel(props: { value: number | null }) {
  const { value } = props;

  return (
    <Box sx={{ display: "flex", alignItems: "center" }}>
      <Box flex="1">
        <LinearProgress
          variant={value === null ? "indeterminate" : "determinate"}
          value={value ?? 0}
          sx={{
            py: 1.2,
            ".MuiLinearProgress-bar": {
              transition: "none",
            },
          }}
        />
      </Box>
      {value !== null && (
        <Box sx={{ minWidth: 35, textAlign: "right", ml: 1 }}>
          <Typography variant="body2" color="text.secondary">{`${Math.round(value)}%`}</Typography>
        </Box>
      )}
    </Box>
  );
}
