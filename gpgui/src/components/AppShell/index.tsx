import {
  Box,
  CssBaseline,
  ThemeProvider,
  createTheme,
  useMediaQuery,
} from "@mui/material";
import React, { Suspense, useMemo } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";

function Loading() {
  console.warn("Loading rendered");
  return (
    <Box
      sx={{
        position: "absolute",
        inset: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      Loading...
    </Box>
  );
}

function AppShell({ children }: { children: React.ReactNode }) {
  const prefersDarkMode = useMediaQuery("(prefers-color-scheme: dark)");
  const theme = useMemo(
    () =>
      createTheme({
        palette: {
          mode: prefersDarkMode ? "dark" : "light",
        },
      }),
    [prefersDarkMode]
  );

  return (
    <React.StrictMode>
      <ThemeProvider theme={theme}>
        <CssBaseline />
        <Suspense fallback={<Loading />}>{children}</Suspense>
      </ThemeProvider>
    </React.StrictMode>
  );
}

export function renderToRoot(children: React.ReactNode) {
  createRoot(document.getElementById("root") as HTMLElement).render(
    <AppShell>{children}</AppShell>
  );
}
