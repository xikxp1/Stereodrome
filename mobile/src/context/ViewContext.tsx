import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useReducer,
} from "react";

export type ViewName =
  | "loading"
  | "connect"
  | "home"
  | "music"
  | "artists"
  | "artist"
  | "albums"
  | "albumList"
  | "album"
  | "songs"
  | "playlists"
  | "playlist"
  | "queue"
  | "downloads"
  | "search"
  | "songContextMenu"
  | "songPlaylistPicker"
  | "nowPlaying"
  | "settings";

export type ViewInstance = {
  name: ViewName;
  title: string;
  params?: Record<string, string>;
};

export type NavigationDirection = "forward" | "back" | "replace";

type ViewContextValue = {
  stack: ViewInstance[];
  current: ViewInstance;
  transitionDirection: NavigationDirection;
  transitionKey: number;
  push(view: ViewInstance): void;
  pop(): void;
  reset(view: ViewInstance): void;
  showNowPlaying(): void;
};

const ViewContext = createContext<ViewContextValue | null>(null);

export const loadingView: ViewInstance = {
  name: "loading",
  title: "Stereodrome",
};
export const connectView: ViewInstance = { name: "connect", title: "Connect" };
export const homeView: ViewInstance = { name: "home", title: "Stereodrome" };
const nowPlaying: ViewInstance = { name: "nowPlaying", title: "Now Playing" };

type ViewState = {
  stack: ViewInstance[];
  transitionDirection: NavigationDirection;
  transitionKey: number;
};

type ViewAction =
  | { type: "push"; view: ViewInstance }
  | { type: "pop" }
  | { type: "reset"; view: ViewInstance }
  | { type: "show_now_playing" };

function transition(
  stack: ViewInstance[],
  direction: NavigationDirection,
  currentKey: number
): ViewState {
  return {
    stack,
    transitionDirection: direction,
    transitionKey: currentKey + 1,
  };
}

function viewReducer(state: ViewState, action: ViewAction): ViewState {
  switch (action.type) {
    case "push":
      return transition(
        [...state.stack, action.view],
        "forward",
        state.transitionKey
      );
    case "pop":
      if (state.stack.length <= 1) {
        return state;
      }
      return transition(state.stack.slice(0, -1), "back", state.transitionKey);
    case "reset":
      return transition([action.view], "replace", state.transitionKey);
    case "show_now_playing": {
      const currentView = state.stack[state.stack.length - 1];
      if (currentView?.name === "nowPlaying") {
        return state;
      }
      return transition(
        [...state.stack, nowPlaying],
        "forward",
        state.transitionKey
      );
    }
    default:
      throw new Error("Unknown view action");
  }
}

export function ViewProvider({ children }: { children: React.ReactNode }) {
  const [state, dispatch] = useReducer(viewReducer, {
    stack: [loadingView],
    transitionDirection: "replace",
    transitionKey: 0,
  });
  const { stack, transitionDirection, transitionKey } = state;
  const current = stack[stack.length - 1] ?? loadingView;

  const push = useCallback((view: ViewInstance) => {
    dispatch({ type: "push", view });
  }, []);

  const pop = useCallback(() => {
    dispatch({ type: "pop" });
  }, []);

  const reset = useCallback((view: ViewInstance) => {
    dispatch({ type: "reset", view });
  }, []);

  const showNowPlaying = useCallback(() => {
    dispatch({ type: "show_now_playing" });
  }, []);

  const value = useMemo(
    () => ({
      stack,
      current,
      transitionDirection,
      transitionKey,
      push,
      pop,
      reset,
      showNowPlaying,
    }),
    [
      current,
      pop,
      push,
      reset,
      showNowPlaying,
      stack,
      transitionDirection,
      transitionKey,
    ]
  );

  return <ViewContext.Provider value={value}>{children}</ViewContext.Provider>;
}

export function useViewStack() {
  const value = useContext(ViewContext);
  if (!value) {
    throw new Error("useViewStack must be used inside ViewProvider");
  }
  return value;
}
