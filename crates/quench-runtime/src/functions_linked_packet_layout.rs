#[derive(Clone, Copy)]
struct LinkedPacketLayout {
    layout: u32,
    link: u16,
    id: u16,
    kind: u16,
    a1: u16,
    a2: u16,
}

struct LinkedPacketWords<'a> {
    link: &'a crate::register_file::SlotWord,
    id: &'a crate::register_file::SlotWord,
    kind: &'a crate::register_file::SlotWord,
    a1: &'a crate::register_file::SlotWord,
    a2: &'a crate::register_file::SlotWord,
}

impl LinkedPacketLayout {
    fn new(packet: &crate::value::ObjectData) -> Option<Self> {
        let slot = |name| {
            writable_own_word(packet, name)?;
            u16::try_from(packet.physical_slot_for_name(name)?).ok()
        };
        Some(Self {
            layout: packet.semantic_layout_id(),
            link: slot("link")?,
            id: slot("id")?,
            kind: slot("kind")?,
            a1: slot("a1")?,
            a2: slot("a2")?,
        })
    }

    fn words<'a>(self, packet: &'a crate::value::ObjectData) -> Option<LinkedPacketWords<'a>> {
        if packet.has_replacement() || packet.semantic_layout_id() != self.layout {
            return None;
        }
        let slots = packet.hot_properties();
        Some(LinkedPacketWords {
            link: slots.slot_word(usize::from(self.link))?,
            id: slots.slot_word(usize::from(self.id))?,
            kind: slots.slot_word(usize::from(self.kind))?,
            a1: slots.slot_word(usize::from(self.a1))?,
            a2: slots.slot_word(usize::from(self.a2))?,
        })
    }


    fn tail_link(
        self,
        head: *const crate::value::ObjectData,
        packet: *const crate::value::ObjectData,
    ) -> Option<*const crate::register_file::SlotWord> {
        if head == packet { return None; }
        let mut slow = Some(head);
        let mut fast = Some(head);
        loop {
            slow = self.advance_optional(slow, packet)?;
            fast = self.advance_optional(self.advance_optional(fast, packet)?, packet)?;
            match (slow, fast) {
                (Some(left), Some(right)) if left == right => return None,
                (None, _) | (_, None) => break,
                _ => {}
            }
        }
        let mut tail = head;
        loop {
            match self.advance(tail, packet)? {
                Some(next) => tail = next,
                None => return self.link_word(tail, packet),
            }
        }
    }

    fn advance_optional(
        self,
        object: Option<*const crate::value::ObjectData>,
        packet: *const crate::value::ObjectData,
    ) -> Option<Option<*const crate::value::ObjectData>> {
        object.map_or(Some(None), |object| self.advance(object, packet))
    }

    fn advance(
        self,
        object: *const crate::value::ObjectData,
        packet: *const crate::value::ObjectData,
    ) -> Option<Option<*const crate::value::ObjectData>> {
        let word = self.link_word(object, packet)?;
        unsafe { &*word }.object_or_null_ptr()
    }

    fn link_word(
        self,
        object: *const crate::value::ObjectData,
        packet: *const crate::value::ObjectData,
    ) -> Option<*const crate::register_file::SlotWord> {
        if object == packet { return None; }
        let object = unsafe { &*object };
        self.words(object).map(|words| std::ptr::from_ref(words.link))
    }
}
