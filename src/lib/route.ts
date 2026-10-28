/** In-app navigation state (no URL router needed in a desktop shell). */
export type Route =
  | { name: "projects" }
  | { name: "new"; preset?: string }
  | { name: "script"; projectId: string }
  | { name: "scenes"; projectId: string }
  | { name: "render"; projectId: string }
  | { name: "settings" };


export type Navigate = (route: Route) => void;
