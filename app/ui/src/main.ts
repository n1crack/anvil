import "./platform";
// The main window, in the order its scripts always ran.
import "./term";
import { popAt } from "./app";
import "./newapp";
import "./git";
import "./logs";
import "./requests";
import "./mail";
import "./services";
import "./share";
import "./packages";
import "./processes";
import "./dev";
import "./containers";
import { sidebarSplitter } from "./split";
import { updateBar } from "./update";

sidebarSplitter();
updateBar(popAt);
