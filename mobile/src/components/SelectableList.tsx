import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  FlatList,
  type NativeScrollEvent,
  type NativeSyntheticEvent,
  Pressable,
  StyleSheet,
  Text,
  View,
  type ListRenderItemInfo,
} from "react-native";
import { ChevronRight, Info, Pencil } from "lucide-react-native";

import type { SongFileState } from "@/types/music";

import { SongFileStateIcon } from "@/components/SongFileStateIcon";
import { useInputBus } from "@/context/InputContext";
import { colors } from "@/components/theme";

const rowHeight = 34;
const edgePaddingRows = 2;

export type SelectableOption = {
  disabled?: boolean;
  label: string;
  kind?: "action" | "editable" | "info";
  fileState?: SongFileState;
  sublabel?: string;
  wheelOnly?: boolean;
  onSelect(): void | Promise<void>;
  onLongSelect?(): void | Promise<void>;
};

type SelectableRowProps = {
  disabled: boolean;
  index: number;
  item: SelectableOption;
  selected: boolean;
  onLongPress(index: number): void;
  onPress(index: number): void;
};

const SelectableRow = memo(
  ({
    disabled,
    index,
    item,
    selected,
    onLongPress,
    onPress,
  }: SelectableRowProps) => (
    <Pressable
      delayLongPress={450}
      disabled={disabled || item.disabled === true}
      onLongPress={() => {
        onLongPress(index);
      }}
      onPress={() => {
        onPress(index);
      }}
      style={[
        styles.row,
        selected && styles.selected,
        item.disabled === true && styles.disabled,
      ]}
    >
      {item.fileState !== undefined ? (
        <SongFileStateIcon
          selected={selected}
          state={item.fileState}
          style={styles.fileStateIcon}
        />
      ) : null}
      <View style={styles.labelGroup}>
        <Text
          numberOfLines={1}
          style={[
            styles.label,
            selected && styles.selectedText,
            item.disabled === true && !selected && styles.disabledText,
          ]}
        >
          {item.label}
        </Text>
        {item.sublabel !== undefined && item.sublabel.length > 0 ? (
          <Text
            numberOfLines={1}
            style={[
              styles.sublabel,
              selected && styles.selectedText,
              item.disabled === true && !selected && styles.disabledText,
            ]}
          >
            {item.sublabel}
          </Text>
        ) : null}
      </View>
      <RowAccessory kind={item.kind ?? "action"} selected={selected} />
    </Pressable>
  ),
  (previous, next) =>
    previous.disabled === next.disabled &&
    previous.index === next.index &&
    previous.item.disabled === next.item.disabled &&
    previous.item.label === next.item.label &&
    previous.item.kind === next.item.kind &&
    previous.item.fileState === next.item.fileState &&
    previous.item.sublabel === next.item.sublabel &&
    previous.item.wheelOnly === next.item.wheelOnly &&
    previous.selected === next.selected
);

function RowAccessory({
  kind,
  selected,
}: {
  kind: NonNullable<SelectableOption["kind"]>;
  selected: boolean;
}) {
  const color = selected ? colors.selectedText : colors.muted;
  const style = styles.accessory;

  if (kind === "editable") {
    return <Pencil color={color} size={14} style={style} />;
  }
  if (kind === "info") {
    return <Info color={color} size={14} style={style} />;
  }
  return <ChevronRight color={color} size={16} style={style} />;
}

export function SelectableList({
  disabled = false,
  options,
  empty = "Nothing here",
  loadingMore = false,
  loadingMoreText = "Loading more",
  onActiveIndexChange,
  onEndReached,
  preserveSelectionOnChange = false,
  resetSelectionKey,
}: {
  options: SelectableOption[];
  disabled?: boolean;
  empty?: string;
  loadingMore?: boolean;
  loadingMoreText?: string;
  onActiveIndexChange?: (
    index: number,
    option: SelectableOption | null
  ) => void;
  onEndReached?: () => void;
  preserveSelectionOnChange?: boolean;
  resetSelectionKey?: string | number | null;
}) {
  const [activeIndex, setActiveIndex] = useState(0);
  const { subscribe } = useInputBus();
  const listRef = useRef<FlatList<SelectableOption>>(null);
  const rowLongPressed = useRef(false);
  const activeIndexRef = useRef(activeIndex);
  const listHeightRef = useRef(0);
  const optionsRef = useRef(options);
  const resetSelectionKeyRef = useRef(resetSelectionKey);
  const scrollOffsetRef = useRef(0);
  const optionsSignature = useMemo(
    () => options.map((option) => option.label).join("\u001e"),
    [options]
  );

  useEffect(() => {
    activeIndexRef.current = activeIndex;
  }, [activeIndex]);

  useEffect(() => {
    optionsRef.current = options;
  }, [options]);

  const resetSelection = useCallback(() => {
    scrollOffsetRef.current = 0;
    activeIndexRef.current = 0;
    setActiveIndex(0);
    listRef.current?.scrollToOffset({
      animated: false,
      offset: 0,
    });
  }, []);

  useEffect(() => {
    onActiveIndexChange?.(activeIndex, options[activeIndex] ?? null);
  }, [activeIndex, onActiveIndexChange, options]);

  const scrollSelectedIntoView = useCallback((index: number) => {
    const listHeight = listHeightRef.current;
    const optionCount = optionsRef.current.length;
    if (listHeight <= 0 || optionCount === 0) {
      return;
    }

    const visibleRows = Math.max(1, Math.floor(listHeight / rowHeight));
    const firstVisibleIndex = Math.floor(scrollOffsetRef.current / rowHeight);
    const lastVisibleIndex = firstVisibleIndex + visibleRows - 1;
    const topThreshold = firstVisibleIndex + edgePaddingRows;
    const bottomThreshold = lastVisibleIndex - edgePaddingRows;
    let nextOffset: number | null = null;

    if (index === 0) {
      nextOffset = 0;
    } else if (index <= topThreshold) {
      nextOffset = (index - edgePaddingRows) * rowHeight;
    } else if (index >= bottomThreshold) {
      nextOffset = (index - visibleRows + edgePaddingRows + 1) * rowHeight;
    }

    if (nextOffset === null) {
      return;
    }

    const maxOffset = Math.max(0, optionCount * rowHeight - listHeight);
    const clampedOffset = Math.max(0, Math.min(nextOffset, maxOffset));
    scrollOffsetRef.current = clampedOffset;
    listRef.current?.scrollToOffset({
      animated: false,
      offset: clampedOffset,
    });
  }, []);

  const activateIndex = useCallback(
    (index: number) => {
      const optionCount = optionsRef.current.length;
      const nextIndex =
        optionCount === 0 ? 0 : Math.max(0, Math.min(index, optionCount - 1));

      if (nextIndex === activeIndexRef.current) {
        return;
      }

      activeIndexRef.current = nextIndex;
      setActiveIndex(nextIndex);
      scrollSelectedIntoView(nextIndex);
    },
    [scrollSelectedIntoView]
  );

  useEffect(() => {
    const shouldResetSelection =
      resetSelectionKeyRef.current !== resetSelectionKey;
    resetSelectionKeyRef.current = resetSelectionKey;

    if (!shouldResetSelection && preserveSelectionOnChange) {
      const clampedIndex =
        options.length === 0
          ? 0
          : Math.min(activeIndexRef.current, options.length - 1);
      activeIndexRef.current = clampedIndex;
      setActiveIndex(clampedIndex);
      requestAnimationFrame(() => {
        scrollSelectedIntoView(clampedIndex);
      });
      return;
    }

    requestAnimationFrame(() => {
      resetSelection();
    });
  }, [
    options.length,
    optionsSignature,
    preserveSelectionOnChange,
    resetSelection,
    resetSelectionKey,
    scrollSelectedIntoView,
  ]);

  useEffect(
    () =>
      subscribe((input) => {
        if (disabled) {
          return;
        }
        if (input === "scroll_forward") {
          activateIndex(
            Math.min(optionsRef.current.length - 1, activeIndexRef.current + 1)
          );
        }
        if (input === "scroll_backward") {
          activateIndex(Math.max(0, activeIndexRef.current - 1));
        }
        if (input === "select") {
          const option = optionsRef.current[activeIndexRef.current];
          if (option !== undefined && option.disabled !== true) {
            void option.onSelect();
          }
        }
        if (input === "select_long") {
          const option = optionsRef.current[activeIndexRef.current];
          if (option !== undefined && option.disabled !== true) {
            void option.onLongSelect?.();
          }
        }
      }),
    [activateIndex, disabled, subscribe]
  );

  const data = useMemo(() => options, [options]);
  const handleRowLongPress = useCallback(
    (index: number) => {
      rowLongPressed.current = true;
      activateIndex(index);
      if (optionsRef.current[index]?.wheelOnly === true) {
        return;
      }
      void optionsRef.current[index]?.onLongSelect?.();
    },
    [activateIndex]
  );
  const handleRowPress = useCallback(
    (index: number) => {
      if (rowLongPressed.current) {
        rowLongPressed.current = false;
        return;
      }

      activateIndex(index);
      if (optionsRef.current[index]?.wheelOnly === true) {
        return;
      }
      void optionsRef.current[index]?.onSelect();
    },
    [activateIndex]
  );
  const renderItem = useCallback(
    ({ item, index }: ListRenderItemInfo<SelectableOption>) => (
      <SelectableRow
        disabled={disabled}
        index={index}
        item={item}
        onLongPress={handleRowLongPress}
        onPress={handleRowPress}
        selected={index === activeIndex}
      />
    ),
    [activeIndex, disabled, handleRowLongPress, handleRowPress]
  );
  const footer = useMemo(
    () =>
      loadingMore ? (
        <View style={styles.footer}>
          <Text style={styles.footerText}>{loadingMoreText}</Text>
        </View>
      ) : undefined,
    [loadingMore, loadingMoreText]
  );

  if (options.length === 0) {
    return (
      <View style={styles.empty}>
        <Text style={styles.emptyText}>{empty}</Text>
      </View>
    );
  }

  return (
    <FlatList
      ref={listRef}
      data={data}
      extraData={activeIndex}
      getItemLayout={(_, index) => ({
        length: rowHeight,
        offset: rowHeight * index,
        index,
      })}
      initialNumToRender={14}
      keyExtractor={(item, index) => `${item.label}-${index}`}
      ListFooterComponent={footer}
      maxToRenderPerBatch={8}
      onEndReached={onEndReached}
      onEndReachedThreshold={0.4}
      onLayout={(event) => {
        listHeightRef.current = event.nativeEvent.layout.height;
        scrollSelectedIntoView(activeIndexRef.current);
      }}
      onScroll={(event: NativeSyntheticEvent<NativeScrollEvent>) => {
        scrollOffsetRef.current = event.nativeEvent.contentOffset.y;
      }}
      onScrollToIndexFailed={(info) => {
        listRef.current?.scrollToOffset({
          animated: false,
          offset: Math.max(0, info.averageItemLength * info.index),
        });
      }}
      removeClippedSubviews
      renderItem={renderItem}
      scrollEventThrottle={64}
      updateCellsBatchingPeriod={50}
      windowSize={5}
    />
  );
}

const styles = StyleSheet.create({
  row: {
    height: rowHeight,
    alignItems: "center",
    flexDirection: "row",
    paddingHorizontal: 8,
  },
  selected: {
    backgroundColor: colors.selected,
  },
  labelGroup: {
    flex: 1,
  },
  fileStateIcon: {
    marginRight: 6,
  },
  accessory: {
    marginLeft: 6,
  },
  label: {
    color: colors.text,
    fontSize: 15,
    fontWeight: "700",
  },
  disabled: {
    opacity: 0.5,
  },
  disabledText: {
    color: colors.muted,
  },
  sublabel: {
    color: colors.muted,
    fontSize: 11,
  },
  selectedText: {
    color: colors.selectedText,
  },
  empty: {
    alignItems: "center",
    flex: 1,
    justifyContent: "center",
  },
  emptyText: {
    color: colors.muted,
    fontWeight: "700",
  },
  footer: {
    alignItems: "center",
    minHeight: rowHeight,
    justifyContent: "center",
  },
  footerText: {
    color: colors.muted,
    fontSize: 12,
    fontWeight: "700",
  },
});
