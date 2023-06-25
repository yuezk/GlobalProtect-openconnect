import { Typography } from "@mui/material";
import { useAtomValue } from "jotai";
import { statusTextAtom } from "../../atoms/status";

export default function StatusText() {
  const statusText = useAtomValue(statusTextAtom);

  return (
    <Typography
      data-tauri-drag-region
      textAlign="center"
      mt={1.5}
      variant="subtitle1"
      paragraph
      sx={{
        overflow: "hidden",
        whiteSpace: "nowrap",
        textOverflow: "ellipsis",
      }}
    >
      {statusText}
    </Typography>
  );
}
