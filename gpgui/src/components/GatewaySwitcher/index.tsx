import { Check } from "@mui/icons-material";
import {
  Drawer,
  ListItemIcon,
  ListItemText,
  MenuItem,
  MenuList,
} from "@mui/material";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import { gatewaySwitcherVisibleAtom } from "../../atoms/gateway";
import {
  GatewayData,
  portalGatewaysAtom,
  selectedGatewayAtom,
  switchToGatewayAtom,
} from "../../atoms/portal";

export default function GatewaySwitcher() {
  const [visible, setGatewaySwitcherVisible] = useAtom(
    gatewaySwitcherVisibleAtom
  );
  const gateways = useAtomValue(portalGatewaysAtom);
  const selectedGateway = useAtomValue(selectedGatewayAtom);
  const switchToGateway = useSetAtom(switchToGatewayAtom);

  const handleClose = () => {
    setGatewaySwitcherVisible(false);
  };

  const handleMenuClick = (gateway: GatewayData) => () => {
    setGatewaySwitcherVisible(false);
    if (gateway.name !== selectedGateway) {
      switchToGateway(gateway);
    }
  };

  return (
    <Drawer anchor="bottom" open={visible} onClose={handleClose}>
      <MenuList
        sx={{
          maxHeight: 320,
        }}
      >
        {!gateways.length && <MenuItem disabled>No gateways found</MenuItem>}
        {gateways.map(({ name, address }) => (
          <MenuItem key={name} onClick={handleMenuClick({ name, address })}>
            {selectedGateway === name && (
              <ListItemIcon>
                <Check />
              </ListItemIcon>
            )}
            <ListItemText inset={selectedGateway !== name}>{name}</ListItemText>
          </MenuItem>
        ))}
      </MenuList>
    </Drawer>
  );
}
