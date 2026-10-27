; ppu.toys TG16 ROM player (HuC6280, ca65 --cpu huc6280).
;
; Plays a frame stream recorded from a toy by `ppu tg16 rom`. This bank is
; ROM bank 0, mapped at $E000 by MPR7 at reset. The exporter patches the
; CONFIG block at $FF00 and appends the stream from bank 1 on. See
; docs/tg16/GUIDE.md ("ROM export") and crates/tg16-core/src/rom.rs for the
; stream format. Rebuild player.bin with ./build.sh after editing.
;
; Memory map while playing:
;   MPR0 = $FF  I/O at $0000 (VDC $0000, VCE $0400, PSG $0800, IRQ $1400)
;   MPR1 = $F8  RAM at $2000 (zero page $2000, stack $2100)
;   MPR2/3      vertical-blank stream window, $4000-$7FFF (two banks)
;   MPR4/5      raster-event window, $8000-$BFFF (two banks)
;   MPR7 = $00  this code
;
; Each frame the VDC's vertical-blank interrupt runs one record: VRAM,
; palette, register and PSG commands, then it arms the raster compare for
; the record's first raster event. Each raster interrupt applies one event
; (scroll, BYR reload, enables, palette writes) and arms the next one.

        .setcpu "huc6280"

; The VDC ports sit below $0100, so every access is written `a:VDC_xx`:
; without it ca65 emits zero-page addressing, which the HuC6280 maps to RAM
; at $2000 instead of the I/O page.
VDC_AR  = $0000         ; VDC address register (write) / status (read)
VDC_DL  = $0002
VDC_DH  = $0003
VCE_CR  = $0400
VCE_AL  = $0402
VCE_AH  = $0403
VCE_DL  = $0404
VCE_DH  = $0405
PSG     = $0800
TMR_CTL = $0C01
IRQ_DIS = $1402
IRQ_ACK = $1403

        .zeropage
ptr:    .res 2          ; vertical-blank stream pointer ($4000-$5FFF)
bank:   .res 1          ; ROM bank mapped at $4000
nxt:    .res 3          ; next record: bank, address
rp:     .res 2          ; raster-event pointer ($8000-$9FFF)
rbank:  .res 1          ; ROM bank mapped at $8000
flags:  .res 1
sr:     .res 1          ; VDC status at interrupt entry
frames: .res 2          ; records played (for debugging)
evflags: .res 1         ; next raster event's flags and BXR, fetched ahead
evbxr:  .res 2          ; so the interrupt can write BXR straight away

        .bss
tiabuf: .res 8          ; tia src, dst, len : rts (self-modified)

        .segment "CONFIG"
; Patched by the exporter (rom.rs, CONFIG_*). Defaults show a blank screen.
cfg_magic:  .byte "PPUTG16R"
cfg_ver:    .byte 1
cfg_vce:    .byte 0             ; VCE control: dot clock
cfg_mwr:    .word $0010         ; VDC $09
cfg_hsr:    .word $0202         ; VDC $0A
cfg_hdr:    .word $041F         ; VDC $0B
cfg_vpr:    .word $1702         ; VDC $0C
cfg_vdw:    .word $00DF         ; VDC $0D
cfg_vcr:    .word $000B         ; VDC $0E
cfg_satb:   .word $7F00         ; VDC $13 (SATB address)
cfg_boot:   .byte 1             ; boot record: bank
            .word $4000         ;              address

        .code
reset:
        sei
        csh                     ; 7.16 MHz
        cld
        ldx #$FF
        txs
        lda #$FF
        tam #$01                ; MPR0 = I/O
        lda #$F8
        tam #$02                ; MPR1 = RAM
        lda #$07
        sta IRQ_DIS             ; mask everything while setting up
        stz TMR_CTL
        stz IRQ_ACK

        lda #$E3                ; tia
        sta tiabuf
        lda #$60                ; rts
        sta tiabuf+7
        stz frames
        stz frames+1

        ; VDC: display and interrupts off while memory is filled.
        st0 #$05
        st1 #$00
        st2 #$00
        st0 #$06
        st1 #$00
        st2 #$00
        st0 #$07
        st1 #$00
        st2 #$00
        st0 #$08
        st1 #$00
        st2 #$00
        ldx #0
@regs:  lda reg_list,x
        sta a:VDC_AR
        ldy reg_src,x
        lda cfg_magic,y
        sta a:VDC_DL
        lda cfg_magic+1,y
        sta a:VDC_DH
        inx
        cpx #reg_count
        bne @regs
        st0 #$0F                ; DCR: repeat SATB DMA every vertical blank
        st1 #$10
        st2 #$00
        lda cfg_vce
        sta VCE_CR

        ; Clear VRAM (32K words).
        st0 #$00
        st1 #$00
        st2 #$00
        st0 #$02
        ldy #$80
        ldx #0
@clr:   st1 #$00
        st2 #$00
        dex
        bne @clr
        dey
        bne @clr

        ; Clear the palette (512 entries).
        stz VCE_AL
        stz VCE_AH
        ldy #2
        ldx #0
@pal:   stz VCE_DL
        stz VCE_DH
        dex
        bne @pal
        dey
        bne @pal

        ; PSG: master off, every channel off, LFO off.
        stz PSG+1
        ldx #5
@psg:   stx PSG
        stz PSG+4
        stz PSG+5
        stz PSG+7
        dex
        bpl @psg
        stz PSG+8
        stz PSG+9

        ; The boot record loads the first frame's memory with the display off.
        lda cfg_boot
        sta nxt
        lda cfg_boot+1
        sta nxt+1
        lda cfg_boot+2
        sta nxt+2
        jsr run_record

        st0 #$13                ; DVSSR: SATB address, DMA from next blank on
        lda cfg_satb
        sta a:VDC_DL
        lda cfg_satb+1
        sta a:VDC_DH
        st0 #$05                ; CR: vertical-blank interrupt, display off
        st1 #$08
        st2 #$00
        lda a:VDC_AR              ; drop anything pending
        lda #$05
        sta IRQ_DIS             ; IRQ1 (VDC) only
        cli
idle:   bra idle

; VDC registers loaded from CONFIG at reset, and their offsets in it.
reg_list:   .byte $09, $0A, $0B, $0C, $0D, $0E
reg_src:    .byte cfg_mwr-cfg_magic, cfg_hsr-cfg_magic, cfg_hdr-cfg_magic
            .byte cfg_vpr-cfg_magic, cfg_vdw-cfg_magic, cfg_vcr-cfg_magic
reg_count = 6

; Raster event at (rp): flags, [BXR lo hi], [n, n x (entry lo hi, color
; lo hi)], [BYR lo hi], [CR lo hi], next RCR lo hi (0 = none).
; The interrupt for RCR = y + 64 arrives in the horizontal blank before line
; y. BXR and palette writes land in time for line y; the VDC has already
; latched BYR and CR for line y by then, so the exporter puts those in the
; event one line earlier.
irq1:
        pha
        lda a:VDC_AR            ; status; reading acknowledges
        sta sr
        and #$04                ; raster compare?
        bne raster
        jmp not_raster
raster:
        ; BXR first, from zero page: the VDC latches it for the line soon
        ; after this interrupt, and a write that straddles the latch (low
        ; byte before, high byte after) shows a wrong scroll for one line.
        bbr0 evflags, @nobxr
        st0 #$07
        lda evbxr
        sta a:VDC_DL
        lda evbxr+1
        sta a:VDC_DH
@nobxr: phx
        phy
        lda evflags
        sta flags
        ldy #1                  ; past the flags
        lsr flags
        bcc @pal
        iny                     ; past BXR, already written
        iny
@pal:   lsr flags
        bcc @nopal
        lda (rp),y
        iny
        tax
@pw:    lda (rp),y
        iny
        sta VCE_AL
        lda (rp),y
        iny
        sta VCE_AH
        lda (rp),y
        iny
        sta VCE_DL
        lda (rp),y
        iny
        sta VCE_DH
        dex
        bne @pw
@nopal: lsr flags
        bcc @nobyr
        st0 #$08
        lda (rp),y
        iny
        sta a:VDC_DL
        lda (rp),y
        iny
        sta a:VDC_DH
@nobyr: lsr flags
        bcc @nocr
        st0 #$05
        lda (rp),y
        iny
        sta a:VDC_DL
        lda (rp),y
        iny
        sta a:VDC_DH
@nocr:  st0 #$06
        lda (rp),y
        iny
        sta a:VDC_DL
        lda (rp),y
        iny
        sta a:VDC_DH
        tya
        clc
        adc rp
        sta rp
        bcc @norm
        inc rp+1
@norm:  lda rp+1
        cmp #$A0
        bcc @mapped
        sbc #$20                ; carry is set
        sta rp+1
        inc rbank
        lda rbank
        tam #$10
        inc a
        tam #$20
@mapped:
        jsr prefetch
        bra check_vblank
not_raster:
        phx
        phy
check_vblank:
        bbr5 sr, @done
        jsr run_record
        inc frames
        bne @done
        inc frames+1
@done:  ply
        plx
        pla
nmi:
irq2:
timer:
        rti

; ---------------------------------------------------------------------------
; Run the record at nxt: header (next bank, next address), commands until
; $00, then the first raster compare value; the raster events follow.
run_record:
        lda nxt
        sta bank
        lda nxt+1
        sta ptr
        lda nxt+2
        sta ptr+1
        jsr map
        ldy #0
        lda (ptr),y
        sta nxt
        iny
        lda (ptr),y
        sta nxt+1
        iny
        lda (ptr),y
        sta nxt+2
        iny
        jsr advance
cmd_loop:
        ldy #0
        lda (ptr),y
        iny
        cmp #0
        beq rec_end
        cmp #1
        beq c_vram
        cmp #2
        beq c_pal
        cmp #3
        beq c_reg
        cmp #4
        bne bad
        jmp c_psg
bad:    bra bad                 ; corrupt stream: stop here

rec_end:
        st0 #$06                ; first raster compare
        lda (ptr),y
        iny
        sta a:VDC_DL
        lda (ptr),y
        iny
        sta a:VDC_DH
        jsr advance
        lda ptr                 ; raster events start here: same bank,
        sta rp                  ; seen through the $8000 window
        lda ptr+1
        clc
        adc #$40
        sta rp+1
        lda bank
        sta rbank
        tam #$10
        inc a
        tam #$20
        ; fall through

; Copy the flags and BXR of the raster event at (rp) to zero page. Past
; the last event this reads stream bytes that are never used.
prefetch:
        lda (rp)
        sta evflags
        ldy #1
        lda (rp),y
        sta evbxr
        iny
        lda (rp),y
        sta evbxr+1
        rts

; $01 addr lo, addr hi, n (0 = 256): n VRAM words
c_vram:
        st0 #$00
        lda (ptr),y
        iny
        sta a:VDC_DL
        lda (ptr),y
        iny
        sta a:VDC_DH
        st0 #$02
        ldx #<VDC_DL
        bra block

; $02 entry lo, entry hi, n (0 = 256): n palette words
c_pal:
        lda (ptr),y
        iny
        sta VCE_AL
        lda (ptr),y
        iny
        sta VCE_AH
        ldx #<VCE_DL
        lda #>VCE_DL
        bra block2

; $03 reg, lo, hi: one VDC register
c_reg:
        lda (ptr),y
        iny
        sta a:VDC_AR
        lda (ptr),y
        iny
        sta a:VDC_DL
        lda (ptr),y
        iny
        sta a:VDC_DH
        jsr advance
        bra run_record_cmd

; Block transfer of n words (count at (ptr),y) to the port pair at
; X (low byte of address) / A (high byte), alternating low and high.
block:  lda #>VDC_DL
block2: stx tiabuf+3
        sta tiabuf+4
        lda (ptr),y
        iny
        stz tiabuf+6
        asl a                   ; bytes = 2n, n = 0 means 256 words
        sta tiabuf+5
        rol tiabuf+6
        ora tiabuf+6
        bne @len
        lda #$02
        sta tiabuf+6
@len:   jsr advance             ; ptr now at the data
        lda ptr
        sta tiabuf+1
        lda ptr+1
        sta tiabuf+2
        jsr tiabuf
        lda ptr
        clc
        adc tiabuf+5
        sta ptr
        lda ptr+1
        adc tiabuf+6
        sta ptr+1
        jsr norm
        bra run_record_cmd

; $04 n, n x (register 0..9, value): PSG writes
c_psg:
        lda (ptr),y
        iny
        sta flags
@w:     lda (ptr),y
        iny
        tax
        lda (ptr),y
        iny
        sta PSG,x
        dec flags
        bne @w
        jsr advance
        ; fall through

run_record_cmd:
        jmp cmd_loop

; ptr += Y, then keep ptr inside $4000-$5FFF by stepping the bank.
advance:
        tya
        clc
        adc ptr
        sta ptr
        bcc norm
        inc ptr+1
norm:   lda ptr+1
        cmp #$60
        bcc adv_ok
        sbc #$20                ; carry is set
        sta ptr+1
        inc bank
map:    lda bank
        tam #$04
        inc a
        tam #$08
adv_ok: rts

        .segment "VECTORS"
        .word irq2              ; $FFF6 IRQ2 / BRK
        .word irq1              ; $FFF8 IRQ1 (VDC)
        .word timer             ; $FFFA timer
        .word nmi               ; $FFFC NMI
        .word reset             ; $FFFE reset
