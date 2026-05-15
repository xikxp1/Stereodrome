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
import { ChevronRight } from "lucide-react-native";

import { useInputBus } from "@/context/InputContext";
import { colors } from "@/components/theme";

const rowHeight = 34;
const edgePaddingRows = 2;

export type SelectableOption = {
  label: string;
  sublabel?: string;
  onSelect(): void | Promise<void>;
  onLongSelect?(): void | Promise<void>;
};

type SelectableRowProps = {
  index: number;
  item: SelectableOption;
  selected: boolean;
  onLongPress(index: number): void;
  onPress(index: number): void;
};

const SelectableRow = memo(
  function SelectableRow({
    index,
    item,
    selected,
    onLongPress,
    onPress,
  }: SelectableRowProps) {
    return (
      <Pressable
        delayLongPress={450}
        onLongPress={() => onLongPress(index)}
        onPress={() => onPress(index)}
        style={[styles.row, selected && styles.selected]}
      >
        <View style={styles.labelGroup}>
          <Text
            numberOfLines={1}
            style={[styles.label, selected && styles.selectedText]}
          >
            {item.label}
          </Text>
          {item.sublabel ? (
            <Text
              numberOfLines={1}
              style={[styles.sublabel, selected && styles.selectedText]}
            >
              {item.sublabel}
            </Text>
          ) : null}
        </View>
        <ChevronRight
          color={selected ? colors.selectedText : colors.muted}
          size={16}
        />
      </Pressable>
    );
  },
  (previous, next) =>
    previous.index === next.index &&
    previous.item.label === next.item.label &&
    previous.item.sublabel === next.item.sublabel &&
    previous.selected === next.selected
);

export function SelectableList({
  options,
  empty = "Nothing here",
  preserveSelectionOnChange = false,
}: {
  options: SelectableOption[];
  empty?: string;
  preserveSelectionOnChange?: boolean;
}) {
  const [activeIndex, setActiveIndex] = useState(0);
  const { subscribe } = useInputBus();
  const listRef = useRef<FlatList<SelectableOption>>(null);
  const rowLongPressed = useRef(false);
  const activeIndexRef = useRef(activeIndex);
  const listHeightRef = useRef(0);
  const optionsRef = useRef(options);
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

  const scrollSelectedIntoView = useCallback((index: number) => {
    const listHeight = listHeightRef.current;
    const optionCount = optionsRef.current.length;
    if (listHeight <= 0 || optionCount === 0) return;

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

    if (nextOffset === null) return;

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
    if (preserveSelectionOnChange) {
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
      scrollOffsetRef.current = 0;
      activeIndexRef.current = 0;
      setActiveIndex(0);
      listRef.current?.scrollToOffset({
        animated: false,
        offset: 0,
      });
    });
  }, [
    options.length,
    optionsSignature,
    preserveSelectionOnChange,
    scrollSelectedIntoView,
  ]);

  useEffect(
    () =>
      subscribe((input) => {
        if (input === "scroll_forward") {
          activateIndex(
            Math.min(optionsRef.current.length - 1, activeIndexRef.current + 1)
          );
        }
        if (input === "scroll_backward") {
          activateIndex(Math.max(0, activeIndexRef.current - 1));
        }
        if (input === "select") {
          void optionsRef.current[activeIndexRef.current]?.onSelect();
        }
        if (input === "select_long") {
          void optionsRef.current[activeIndexRef.current]?.onLongSelect?.();
        }
      }),
    [activateIndex, subscribe]
  );

  const data = useMemo(() => options, [options]);
  const handleRowLongPress = useCallback(
    (index: number) => {
      rowLongPressed.current = true;
      activateIndex(index);
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
      void optionsRef.current[index]?.onSelect();
    },
    [activateIndex]
  );
  const renderItem = useCallback(
    ({ item, index }: ListRenderItemInfo<SelectableOption>) => (
      <SelectableRow
        index={index}
        item={item}
        onLongPress={handleRowLongPress}
        onPress={handleRowPress}
        selected={index === activeIndex}
      />
    ),
    [activeIndex, handleRowLongPress, handleRowPress]
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
      maxToRenderPerBatch={8}
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
      scrollEventThrottle={16}
      updateCellsBatchingPeriod={16}
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
  label: {
    color: colors.text,
    fontSize: 15,
    fontWeight: "700",
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
});
