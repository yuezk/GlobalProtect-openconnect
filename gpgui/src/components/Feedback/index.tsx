import { BugReport, Favorite } from "@mui/icons-material";
import { Chip, ChipProps, Stack } from "@mui/material";
import { red } from "@mui/material/colors";

const LinkChip = (props: ChipProps<"a">) => (
  <Chip
    component="a"
    target="_blank"
    clickable
    variant="outlined"
    size="small"
    {...props}
  />
);

export default function Feedback() {
  return (
    <Stack
      direction="row"
      justifyContent="space-evenly"
      mt={1}
      data-tauri-drag-region
    >
      <LinkChip
        avatar={<BugReport />}
        label="Feedback"
        href="https://github.com/yuezk/GlobalProtect-openconnect/issues"
      />
      <LinkChip
        avatar={<Favorite />}
        label="Donate"
        href="https://www.buymeacoffee.com/yuezk"
        sx={{
          "& .MuiSvgIcon-root": {
            color: red[300],
            transition: "all 0.3s ease",
          },
          "&:hover": {
            ".MuiSvgIcon-root": {
              color: red[500],
              transform: "scale(1.1)",
            },
          },
        }}
      />
    </Stack>
  );
}
