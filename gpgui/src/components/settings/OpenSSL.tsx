import { TabPanel } from "@mui/lab";
import {
  Alert,
  Box,
  Checkbox,
  FormControlLabel,
  TextField,
} from "@mui/material";
import { useAtom, useAtomValue } from "jotai";
import { customOpenSSLAtom, opensslConfigAtom } from "../../atoms/settings";

export default function OpenSSL() {
  const [customOpenSSL, setCustomOpenSSL] = useAtom(customOpenSSLAtom);
  const opensslConfig = useAtomValue(opensslConfigAtom);

  function handleCustomOpenSSLChange(
    event: React.ChangeEvent<HTMLInputElement>
  ) {
    setCustomOpenSSL(event.target.checked);
  }

  return (
    <TabPanel value="openssl">
      <Alert severity="info">
        You need to enable this if you encountered the "Unsafe Legacy
        Renegotiation" error.
      </Alert>

      <Box mt={2}>
        <FormControlLabel
          control={
            <Checkbox
              checked={customOpenSSL}
              onChange={handleCustomOpenSSLChange}
            />
          }
          label="Use custom OpenSSL configuration"
        />

        {customOpenSSL && (
          <TextField
            value={opensslConfig}
            fullWidth
            multiline
            InputProps={{
              readOnly: true,
            }}
            sx={{
              mb: 1,
              "& textarea": {
                fontFamily: "monospace",
                fontSize: 14,
                lineHeight: 1.2,
              },
            }}
          />
        )}

        <Alert severity="warning">
          You need to restart the client after changing this setting.
        </Alert>
      </Box>
    </TabPanel>
  );
}
