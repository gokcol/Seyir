//! Crop and flip the working image without changing the disk source.
use super::{Canvas, Decoded, Loaded, Reply, Viewer, canvas, egui, post};
use crate::crop::{CropAction, CropSelection};
use std::sync::Arc;

pub(super) struct CropSession {
    preview: Arc<Decoded>,
    texture: egui::TextureHandle,
    selection: CropSelection,
}

pub(super) struct UndoImage {
    picture: (Arc<Decoded>, egui::TextureHandle),
    canvas: Canvas,
}

impl Viewer {
    pub(super) fn clear_image_edits(&mut self) {
        self.original = None;
        self.image_undo = None;
        self.editing_image = false;
        self.crop = None;
    }

    pub(super) fn has_pixel_edits(&self) -> bool {
        self.original
            .as_ref()
            .zip(self.picture.as_ref())
            .is_some_and(|(original, (image, _))| !Arc::ptr_eq(original, image))
    }

    pub(super) fn can_transform_image(&self) -> bool {
        self.picture.is_some() && !self.editing_image && !self.saving_image && !self.info.saving
    }

    pub(super) fn start_crop(&mut self, ctx: &egui::Context) {
        if !self.can_transform_image() {
            return;
        }
        let Some((image, texture)) = &self.picture else {
            return;
        };
        self.editing_image = true;
        if self.canvas.angle == 0. {
            self.crop = Some(CropSession {
                preview: image.clone(),
                texture: texture.clone(),
                selection: CropSelection::default(),
            });
            return;
        }
        let image = image.clone();
        let angle = self.canvas.angle;
        let token = self.image_request;
        let max_side = self.max_texture_side;
        let post = post(self.sender.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = canvas::oriented_pixels(&image.pixels, angle).map(|pixels| {
                Loaded::new(
                    Arc::new(Decoded {
                        pixels,
                        format: image.format.clone(),
                    }),
                    max_side,
                )
            });
            post(Reply::CropPreview(token, result));
        });
    }

    pub(super) fn receive_crop(&mut self, ctx: &egui::Context, result: Result<Loaded, String>) {
        match result {
            Ok(loaded) => {
                let texture = self.texture(ctx, &loaded);
                self.crop = Some(CropSession {
                    preview: loaded.image,
                    texture,
                    selection: CropSelection::default(),
                });
            }
            Err(error) => {
                self.editing_image = false;
                self.report(Err(error));
            }
        }
    }

    pub(super) fn show_crop(&mut self, ctx: &egui::Context) {
        let Some(crop) = &mut self.crop else {
            return;
        };
        let size = [crop.preview.pixels.width(), crop.preview.pixels.height()];
        let Some(action) = crop.selection.show(ctx, crop.texture.id(), size) else {
            return;
        };
        let crop = self.crop.take().expect("active crop");
        let CropAction::Apply {
            x,
            y,
            width,
            height,
        } = action
        else {
            self.editing_image = false;
            return;
        };
        let max_side = self.max_texture_side;
        let token = self.image_request;
        let post = post(self.sender.clone(), ctx.clone());
        std::thread::spawn(move || {
            let pixels =
                image::imageops::crop_imm(&crop.preview.pixels, x, y, width, height).to_image();
            let image = Arc::new(Decoded {
                pixels,
                format: crop.preview.format.clone(),
            });
            post(Reply::ImageEdited(token, Ok(Loaded::new(image, max_side))));
        });
    }

    pub(super) fn flip_image(&mut self, ctx: &egui::Context, horizontal: bool) {
        if !self.can_transform_image() {
            return;
        }
        let Some(image) = self.output_image() else {
            return;
        };
        self.editing_image = true;
        let token = self.image_request;
        let max_side = self.max_texture_side;
        let post = post(self.sender.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result =
                canvas::oriented_pixels(&image.source.pixels, image.angle).map(|mut pixels| {
                    if horizontal {
                        image::imageops::flip_horizontal_in_place(&mut pixels);
                    } else {
                        image::imageops::flip_vertical_in_place(&mut pixels);
                    }
                    Loaded::new(
                        Arc::new(Decoded {
                            pixels,
                            format: image.source.format.clone(),
                        }),
                        max_side,
                    )
                });
            post(Reply::ImageEdited(token, result));
        });
    }

    pub(super) fn receive_image_edit(
        &mut self,
        ctx: &egui::Context,
        result: Result<Loaded, String>,
    ) {
        self.editing_image = false;
        match result {
            Ok(loaded) => {
                let texture = self.texture(ctx, &loaded);
                let previous = self.picture.replace((loaded.image, texture));
                let canvas = std::mem::take(&mut self.canvas);
                self.image_undo = previous.map(|picture| UndoImage { picture, canvas });
                self.transition = None;
                self.report(Ok("Image edit applied.".into()));
            }
            Err(error) => self.report(Err(error)),
        }
    }

    pub(super) fn undo_image_edit(&mut self) {
        if self.can_transform_image()
            && let Some(previous) = self.image_undo.take()
        {
            self.picture = Some(previous.picture);
            self.canvas = previous.canvas;
            self.transition = None;
            self.report(Ok("Image edit undone.".into()));
        }
    }
}
