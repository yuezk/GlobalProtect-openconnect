import { TabPanel } from "@mui/lab";
import { Alert, Box, Link, TextField, Typography } from "@mui/material";
import { useAtom } from "jotai";
import { openconnectConfigAtom } from "../../atoms/settings";

export default function OpenConnect() {
  const [openconnectConfig, setOpenconnectConfig] = useAtom(
    openconnectConfigAtom
  );

  return (
    <TabPanel
      value="openconnect"
      sx={{ flex: 1, display: "flex", flexDirection: "column" }}
    >
      <Alert severity="info">
        You can edit the OpenConnect parameters here. More information can be
        found{" "}
        <Link
          target="_blank"
          href="https://github.com/yuezk/GlobalProtect-openconnect/wiki/Configuration"
        >
          here
        </Link>
        .
      </Alert>

      <Box mt={2} sx={{ flex: 1, display: "flex", flexDirection: "column" }}>
        <Typography variant="subtitle1">
          File location: /etc/gpservice/gp.conf
        </Typography>
        <TextField
          fullWidth
          multiline
          value={openconnectConfig}
          onChange={(event) => setOpenconnectConfig(event.target.value)}
          sx={{
            flex: 1,
            display: "flex",
            "& .MuiInputBase-root": {
              flex: "1 1 auto",
              display: "flex",
              flexDirection: "column",
              height: 0,

              "& textarea": {
                whiteSpace: "pre",
                fontFamily: "monospace",
                overflow: "auto !important",
                fontSize: 14,
                lineHeight: 1.2,
              },
            },
          }}
        />
      </Box>
    </TabPanel>
  );
}
