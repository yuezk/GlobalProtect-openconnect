import { Box, CssBaseline, ThemeProvider } from "@mui/material";
import { SnackbarProvider } from "notistack";
import React, { Suspense } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";
import useGlobalTheme from "./useGlobalTheme";

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
  const theme = useGlobalTheme();

  return (
    <React.StrictMode>
      <ThemeProvider theme={theme}>
        <SnackbarProvider>
          <CssBaseline />
          <Suspense fallback={<Loading />}>{children}</Suspense>
        </SnackbarProvider>
      </ThemeProvider>
    </React.StrictMode>
  );
}

export function renderToRoot(children: React.ReactNode) {
  createRoot(document.getElementById("root") as HTMLElement).render(
    <AppShell>{children}</AppShell>
  );
}
