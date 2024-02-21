import { createRoot } from "react-dom/client"
import App from "../components/App/App";

const rootApp = createRoot(document.getElementById('root') as HTMLElement);

rootApp.render(<App />);
