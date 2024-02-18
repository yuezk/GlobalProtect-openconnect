import {
  Box,
  Button,
  CssBaseline,
  LinearProgress,
  LinearProgressProps,
  Typography,
} from "@mui/material";

import "./styles.css";

import logo from "../../assets/icon.svg";

export default function App() {
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
            sx={{ width: 64, height: 64 }}
            data-tauri-drag-region
          />
          <Box flex={1} ml={2}>
            <DownloadIndicator />
            {/* <DownloadFailed /> */}
          </Box>
        </Box>
      </Box>
    </>
  );
}

function DownloadIndicator() {
  return (
    <>
      <Typography variant="h1" fontSize="1rem" data-tauri-drag-region>
        Updating the GUI components...
      </Typography>
      <Box mt={1}>
        <LinearProgressWithLabel value={50} />
      </Box>
    </>
  );
}

function DownloadFailed() {
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

function LinearProgressWithLabel(props: LinearProgressProps & { value: number }) {
  return (
    <Box sx={{ display: "flex", alignItems: "center" }}>
      <Box sx={{ width: "100%", mr: 1 }}>
        <LinearProgress variant="determinate" {...props} />
      </Box>
      <Box sx={{ minWidth: 35 }}>
        <Typography variant="body2" color="text.secondary">{`${Math.round(
          props.value,
        )}%`}</Typography>
      </Box>
    </Box>
  );
}
