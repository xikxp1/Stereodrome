import { listen, type UnlistenFn } from "@tauri-apps/api/event";

interface SpectrumData {
  bands: number[];
  peak: number;
}

class SpectrumStore {
  // Spectrum band values (0.0 to 1.0)
  bands = $state<number[]>([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
  peak = $state(0);

  // Visualizer visibility
  enabled = $state(true);

  // Event listener
  private unlistenSpectrum: UnlistenFn | null = null;

  constructor() {
    this.setupEventListener();
  }

  private async setupEventListener() {
    this.unlistenSpectrum = await listen<SpectrumData>(
      "spectrum-data",
      (event) => {
        if (this.enabled) {
          this.bands = event.payload.bands;
          this.peak = event.payload.peak;
        }
      }
    );
  }

  toggle() {
    this.enabled = !this.enabled;
    if (!this.enabled) {
      // Reset bands when disabled
      this.bands = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
      this.peak = 0;
    }
  }

  // Cleanup
  destroy() {
    if (this.unlistenSpectrum) {
      this.unlistenSpectrum();
    }
  }
}

export const spectrum = new SpectrumStore();
