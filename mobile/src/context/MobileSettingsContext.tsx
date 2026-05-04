import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { Directory, File, Paths } from "expo-file-system";

type ButtonHandedness = "right" | "left";

type MobileSettings = {
  buttonHandedness: ButtonHandedness;
};

type MobileSettingsContextValue = {
  buttonHandedness: ButtonHandedness;
  setButtonHandedness(value: ButtonHandedness): void;
  toggleButtonHandedness(): void;
};

const settingsDirectory = new Directory(Paths.document, "stereodrome");
const settingsFile = new File(settingsDirectory, "mobile-settings.json");
const defaultSettings: MobileSettings = {
  buttonHandedness: "right",
};

const MobileSettingsContext =
  createContext<MobileSettingsContextValue | null>(null);

function parseSettings(raw: string): MobileSettings {
  const parsed = JSON.parse(raw) as Partial<MobileSettings>;
  return {
    buttonHandedness:
      parsed.buttonHandedness === "left"
        ? "left"
        : defaultSettings.buttonHandedness,
  };
}

function persistSettings(settings: MobileSettings) {
  settingsDirectory.create({ idempotent: true, intermediates: true });
  settingsFile.write(JSON.stringify(settings));
}

export function MobileSettingsProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const [settings, setSettings] = useState<MobileSettings>(defaultSettings);

  useEffect(() => {
    let mounted = true;

    async function load() {
      try {
        if (!settingsFile.exists) {
          return;
        }
        const nextSettings = parseSettings(await settingsFile.text());
        if (mounted) {
          setSettings(nextSettings);
        }
      } catch {
        if (mounted) {
          setSettings(defaultSettings);
        }
      }
    }

    void load();
    return () => {
      mounted = false;
    };
  }, []);

  const setButtonHandedness = useCallback((value: ButtonHandedness) => {
    setSettings((current) => {
      const nextSettings = { ...current, buttonHandedness: value };
      persistSettings(nextSettings);
      return nextSettings;
    });
  }, []);

  const toggleButtonHandedness = useCallback(() => {
    setSettings((current) => {
      const nextSettings: MobileSettings = {
        ...current,
        buttonHandedness:
          current.buttonHandedness === "right" ? "left" : "right",
      };
      persistSettings(nextSettings);
      return nextSettings;
    });
  }, []);

  const value = useMemo(
    () => ({
      buttonHandedness: settings.buttonHandedness,
      setButtonHandedness,
      toggleButtonHandedness,
    }),
    [setButtonHandedness, settings.buttonHandedness, toggleButtonHandedness]
  );

  return (
    <MobileSettingsContext.Provider value={value}>
      {children}
    </MobileSettingsContext.Provider>
  );
}

export function useMobileSettings() {
  const value = useContext(MobileSettingsContext);
  if (!value) {
    throw new Error(
      "useMobileSettings must be used inside MobileSettingsProvider"
    );
  }
  return value;
}
