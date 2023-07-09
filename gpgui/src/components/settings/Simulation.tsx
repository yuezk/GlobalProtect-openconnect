import { TabPanel } from "@mui/lab";
import {
  Alert,
  Box,
  FormControl,
  FormControlLabel,
  FormLabel,
  Radio,
  RadioGroup,
  TextField,
} from "@mui/material";
import { useAtom, useAtomValue } from "jotai";
import {
  clientOSAtom,
  clientVersionAtom,
  defaultOsVersionAtom,
  osVersionAtom,
  userAgentAtom,
} from "../../atoms/settings";
import {
  ClientOS,
  DEFAULT_CLIENT_VERSION,
} from "../../services/settingsService";

export default function Simulation() {
  const [clientOS, setClientOS] = useAtom(clientOSAtom);
  const [osVersion, setOsVersion] = useAtom(osVersionAtom);
  const [clientVersion, setClientVersion] = useAtom(clientVersionAtom);
  const defaultOsVersion = useAtomValue(defaultOsVersionAtom);
  const userAgent = useAtomValue(userAgentAtom);

  const handleClientOSChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setClientOS(event.target.value as ClientOS);
  };

  return (
    <TabPanel value="simulation">
      <Alert severity="info">
        Controls the platform the client should simulate.
      </Alert>

      <Box
        mt={2}
        sx={{
          "& > .MuiFormControl-root": {
            mb: 2,
          },
        }}
      >
        <FormControl>
          <FormLabel>Client OS</FormLabel>
          <RadioGroup row value={clientOS} onChange={handleClientOSChange}>
            <FormControlLabel value="Linux" control={<Radio />} label="Linux" />
            <FormControlLabel
              value="Windows"
              control={<Radio />}
              label="Windows"
            />
            <FormControlLabel value="Mac" control={<Radio />} label="macOS" />
          </RadioGroup>
        </FormControl>
        <TextField
          label="OS Version"
          InputLabelProps={{ shrink: true }}
          variant="standard"
          value={osVersion}
          onChange={(event) => setOsVersion(event.target.value)}
          fullWidth
          size="small"
          placeholder={`Default: ${defaultOsVersion}`}
        />
        <TextField
          label="Client Version"
          InputLabelProps={{ shrink: true }}
          variant="standard"
          onChange={(event) => setClientVersion(event.target.value)}
          value={clientVersion}
          fullWidth
          size="small"
          placeholder={`Default: ${DEFAULT_CLIENT_VERSION}`}
        />
        <TextField
          label="User Agent"
          InputLabelProps={{ shrink: true }}
          variant="standard"
          value={userAgent}
          fullWidth
          size="small"
          disabled
          multiline
        />
      </Box>
    </TabPanel>
  );
}
