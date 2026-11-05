import { useState } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../../a11y/announcer";
import { toEngineError } from "../../../api";
import { ModalDialog } from "../components/ModalDialog";
import { pickOpenPath } from "../fileDialogs";
import { ownImageNumber, type OwnImages } from "./useOwnImages";

const IMAGE_FILTER_EXTENSIONS = ["png", "jpg", "jpeg", "webp"];

/**
 * Settings → Camera → Manage images (FR-8.15): the own background images with a thumbnail and
 * Delete each, plus Add image. `onDeleted` lets the page drop a default background that pointed
 * at the deleted image.
 */
export function ManageImagesDialog({
  ownImages,
  onDeleted,
  onClose,
}: {
  ownImages: OwnImages;
  onDeleted: (id: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("settings");
  const [error, setError] = useState<string | null>(null);
  const images = ownImages.images ?? [];
  const nameOf = (id: string) => t("camera.imageName", { number: ownImageNumber(images, id) });

  const addImage = async () => {
    setError(null);
    const path = await pickOpenPath(
      { name: t("camera.imageFilter"), extensions: IMAGE_FILTER_EXTENSIONS },
      t("camera.addImageTitle"),
    );
    if (path === null) return;
    try {
      await ownImages.add(path);
      announce(
        t("camera.imageAdded", { name: t("camera.imageName", { number: images.length + 1 }) }),
      );
    } catch (failure) {
      if (toEngineError(failure).code !== "invalidInput") {
        console.error("Adding a background image failed", failure);
      }
      setError(t("camera.imageImportFailed"));
    }
  };

  const deleteImage = async (id: string) => {
    setError(null);
    const name = nameOf(id);
    try {
      await ownImages.remove(id);
      onDeleted(id);
      announce(t("camera.imageDeleted", { name }));
    } catch (failure) {
      console.error("Deleting a background image failed", failure);
      setError(toEngineError(failure).message);
    }
  };

  return (
    <ModalDialog title={t("camera.imagesTitle")} onClose={onClose}>
      {images.length === 0 ? (
        <p className="settings-hint">{t("camera.imagesEmpty")}</p>
      ) : (
        <ul className="settings-camera-images">
          {images.map((image) => (
            <li key={image.id} className="settings-camera-images__item">
              <img className="settings-camera-images__thumb" src={image.thumbnail} alt="" />
              <span className="settings-camera-images__name">{nameOf(image.id)}</span>
              <button
                type="button"
                className="settings-button"
                aria-label={t("camera.imageDeleteLabel", { name: nameOf(image.id) })}
                onClick={() => {
                  void deleteImage(image.id);
                }}
              >
                {t("camera.imageDelete")}
              </button>
            </li>
          ))}
        </ul>
      )}
      {error !== null && (
        <p className="settings-error" role="alert">
          {error}
        </p>
      )}
      <div className="settings-camera-images__actions">
        <button
          type="button"
          className="settings-button"
          onClick={() => {
            void addImage();
          }}
        >
          {t("camera.addImage")}
        </button>
        <button
          type="button"
          className="settings-button settings-button--primary"
          onClick={onClose}
        >
          {t("camera.close")}
        </button>
      </div>
    </ModalDialog>
  );
}
